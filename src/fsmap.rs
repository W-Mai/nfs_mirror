use std::collections::{BTreeSet, HashMap};
use std::ffi::{OsStr, OsString};
use std::fs::Metadata;
use std::os::unix::ffi::OsStrExt;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use intaglio::Symbol;
use intaglio::osstr::SymbolTable;
use tokio::fs;
use tracing::debug;

use zerofs_nfsserve::fs_util::*;
use zerofs_nfsserve::nfs::*;

#[derive(Debug, Clone)]
pub struct FSEntry {
    pub name: Vec<Symbol>,
    pub fsmeta: fattr3,
    /// metadata when building the children list
    pub children_meta: fattr3,
    pub children: Option<BTreeSet<fileid3>>,
}

/// File system mapping structure
#[derive(Debug)]
pub struct FSMap {
    /// Mount configurations
    pub mounts: Vec<(String, PathBuf, bool)>, // (target_path, source_path, read_only)
    /// Next file ID counter
    pub next_fileid: AtomicU64,
    /// Symbol table for interned strings
    pub intern: SymbolTable,
    /// Mapping from file ID to file system entry
    pub id_to_path: HashMap<fileid3, FSEntry>,
    /// Mapping from path symbols to file ID
    pub path_to_id: HashMap<Vec<Symbol>, fileid3>,
}

pub enum RefreshResult {
    /// The fileid was deleted
    Delete,
    /// The fileid needs to be reloaded. mtime has been updated, caches
    /// need to be evicted.
    Reload,
    /// Nothing has changed
    Noop,
}

impl FSMap {
    /// Create a new FSMap with root directory only
    pub fn new_with_root(root_dir: PathBuf) -> FSMap {
        let mut fsmap = FSMap {
            mounts: Vec::new(),
            next_fileid: AtomicU64::new(1),
            intern: SymbolTable::new(),
            id_to_path: HashMap::new(),
            path_to_id: HashMap::new(),
        };

        // Create root entry with actual root directory metadata
        let root_metadata = root_dir.metadata().unwrap_or_else(|_| {
            // Create default metadata if root doesn't exist
            std::fs::metadata(".").unwrap()
        });

        let root_entry = FSEntry {
            name: Vec::new(),
            fsmeta: metadata_to_fattr3(0, &root_metadata),
            children_meta: metadata_to_fattr3(0, &root_metadata),
            children: Some(BTreeSet::new()),
        };

        fsmap.id_to_path.insert(0, root_entry);
        fsmap.path_to_id.insert(Vec::new(), 0);

        fsmap
    }

    /// Create a new FSMap with mount points
    pub fn new_with_mounts(root_dir: PathBuf, mounts: Vec<(String, PathBuf, bool)>) -> FSMap {
        let mut fsmap = FSMap {
            mounts,
            next_fileid: AtomicU64::new(1),
            intern: SymbolTable::new(),
            id_to_path: HashMap::new(),
            path_to_id: HashMap::new(),
        };

        // Create root entry with actual root directory metadata
        let root_metadata = root_dir.metadata().unwrap_or_else(|_| {
            // Create default metadata if root doesn't exist
            std::fs::metadata(".").unwrap()
        });

        let root_entry = FSEntry {
            name: Vec::new(),
            fsmeta: metadata_to_fattr3(0, &root_metadata),
            children_meta: metadata_to_fattr3(0, &root_metadata),
            children: Some(BTreeSet::new()),
        };

        fsmap.id_to_path.insert(0, root_entry);
        fsmap.path_to_id.insert(Vec::new(), 0);

        // Initialize mount points as root children
        for (target_path, source_path, _read_only) in &fsmap.mounts {
            let target_sym = fsmap
                .intern
                .intern(OsStr::new(target_path.trim_start_matches('/')).to_os_string())
                .unwrap();

            let mount_entry = FSEntry {
                name: vec![target_sym],
                fsmeta: metadata_to_fattr3(
                    1,
                    &source_path.metadata().unwrap_or_else(|_| {
                        // Create default metadata if source doesn't exist
                        std::fs::metadata(".").unwrap()
                    }),
                ),
                children_meta: metadata_to_fattr3(
                    1,
                    &source_path
                        .metadata()
                        .unwrap_or_else(|_| std::fs::metadata(".").unwrap()),
                ),
                children: None,
            };

            let fileid = fsmap.next_fileid.fetch_add(1, Ordering::SeqCst) as fileid3;
            fsmap.id_to_path.insert(fileid, mount_entry);
            fsmap.path_to_id.insert(vec![target_sym], fileid);

            // Add to root children
            if let Some(root_entry) = fsmap.id_to_path.get_mut(&0) {
                if let Some(ref mut children) = root_entry.children {
                    children.insert(fileid);
                }
            }
        }

        fsmap
    }

    /// Get the actual file system path for a given symbolic path
    pub async fn sym_to_real_path(&self, symlist: &[Symbol]) -> Option<(PathBuf, bool)> {
        if symlist.is_empty() {
            return None; // Root path doesn't map to a real file
        }

        // Check if this is a mount point
        if symlist.len() == 1 {
            let mount_name = self.intern.get(symlist[0])?;
            for (target_path, source_path, _read_only) in &self.mounts {
                if mount_name == OsStr::new(target_path.trim_start_matches('/')) {
                    return Some((source_path.clone(), *_read_only));
                }
            }
        }

        // Check if this is under a mount point
        if symlist.len() >= 1 {
            let mount_name = self.intern.get(symlist[0])?;
            for (target_path, source_path, _read_only) in &self.mounts {
                if mount_name == OsStr::new(target_path.trim_start_matches('/')) {
                    let mut real_path = source_path.clone();
                    for sym in &symlist[1..] {
                        real_path.push(self.intern.get(*sym)?);
                    }
                    return Some((real_path, *_read_only));
                }
            }
        }

        None
    }

    pub async fn sym_to_path(&self, symlist: &[Symbol]) -> PathBuf {
        let mut ret = PathBuf::new();
        for i in symlist.iter() {
            ret.push(self.intern.get(*i).unwrap());
        }
        ret
    }

    pub async fn sym_to_fname(&self, symlist: &[Symbol]) -> OsString {
        if let Some(x) = symlist.last() {
            self.intern.get(*x).unwrap().into()
        } else {
            "".into()
        }
    }

    fn collect_all_children(&self, id: fileid3, ret: &mut Vec<fileid3>) {
        ret.push(id);
        if let Some(entry) = self.id_to_path.get(&id) {
            if let Some(ref ch) = entry.children {
                for i in ch.iter() {
                    self.collect_all_children(*i, ret);
                }
            }
        }
    }

    pub fn delete_entry(&mut self, id: fileid3) {
        let mut children = Vec::new();
        self.collect_all_children(id, &mut children);
        for i in children.iter() {
            if let Some(ent) = self.id_to_path.remove(i) {
                self.path_to_id.remove(&ent.name);
            }
        }
    }

    pub fn find_entry(&self, id: fileid3) -> Result<FSEntry, nfsstat3> {
        Ok(self
            .id_to_path
            .get(&id)
            .ok_or(nfsstat3::NFS3ERR_NOENT)?
            .clone())
    }

    pub fn find_entry_mut(&mut self, id: fileid3) -> Result<&mut FSEntry, nfsstat3> {
        self.id_to_path.get_mut(&id).ok_or(nfsstat3::NFS3ERR_NOENT)
    }

    pub async fn find_child(&self, id: fileid3, filename: &[u8]) -> Result<fileid3, nfsstat3> {
        let mut name = self
            .id_to_path
            .get(&id)
            .ok_or(nfsstat3::NFS3ERR_NOENT)?
            .name
            .clone();
        name.push(
            self.intern
                .check_interned(OsStr::from_bytes(filename))
                .ok_or(nfsstat3::NFS3ERR_NOENT)?,
        );
        Ok(*self.path_to_id.get(&name).ok_or(nfsstat3::NFS3ERR_NOENT)?)
    }

    pub async fn refresh_entry(&mut self, id: fileid3) -> Result<RefreshResult, nfsstat3> {
        let entry = self
            .id_to_path
            .get(&id)
            .ok_or(nfsstat3::NFS3ERR_NOENT)?
            .clone();

        // Get the real file system path
        let (real_path, _read_only) = match self.sym_to_real_path(&entry.name).await {
            Some(path) => path,
            None => {
                // Root entry or mount point, handle differently
                if entry.name.is_empty() {
                    // Root entry - always exists
                    return Ok(RefreshResult::Noop);
                } else {
                    // Mount point - check if source exists
                    let mounts = self.mounts.clone();
                    for (target_path, source_path, _) in &mounts {
                        if entry.name.len() == 1 {
                            let mount_name = self
                                .intern
                                .get(entry.name[0])
                                .ok_or(nfsstat3::NFS3ERR_NOENT)?;
                            if mount_name == OsStr::new(target_path.trim_start_matches('/')) {
                                if !source_path.exists() {
                                    self.delete_entry(id);
                                    debug!(
                                        "Deleting mount point {:?}: {:?}. Ent: {:?}",
                                        id, source_path, entry
                                    );
                                    return Ok(RefreshResult::Delete);
                                }
                                let meta = fs::symlink_metadata(source_path)
                                    .await
                                    .map_err(|_| nfsstat3::NFS3ERR_IO)?;
                                let meta = metadata_to_fattr3(id, &meta);
                                if fattr3_differ(&meta, &entry.fsmeta) {
                                    self.id_to_path.get_mut(&id).unwrap().fsmeta = meta;
                                    debug!(
                                        "Reloading mount point {:?}: {:?}. Ent: {:?}",
                                        id, source_path, entry
                                    );
                                    return Ok(RefreshResult::Reload);
                                }
                                return Ok(RefreshResult::Noop);
                            }
                        }
                    }
                    return Ok(RefreshResult::Noop);
                }
            }
        };

        if !exists_no_traverse(&real_path) {
            self.delete_entry(id);
            debug!(
                "Deleting entry A {:?}: {:?}. Ent: {:?}",
                id, real_path, entry
            );
            return Ok(RefreshResult::Delete);
        }

        let meta = fs::symlink_metadata(&real_path)
            .await
            .map_err(|_| nfsstat3::NFS3ERR_IO)?;
        let meta = metadata_to_fattr3(id, &meta);
        if !fattr3_differ(&meta, &entry.fsmeta) {
            return Ok(RefreshResult::Noop);
        }
        // If we get here we have modifications
        if entry.fsmeta.ftype as u32 != meta.ftype as u32 {
            // if the file type changed ex: file->dir or dir->file
            // really the entire file has been replaced.
            // we expire the entire id
            debug!(
                "File Type Mismatch FT {:?} : {:?} vs {:?}",
                id, entry.fsmeta.ftype, meta.ftype
            );
            debug!(
                "File Type Mismatch META {:?} : {:?} vs {:?}",
                id, entry.fsmeta, meta
            );
            self.delete_entry(id);
            debug!(
                "Deleting entry B {:?}: {:?}. Ent: {:?}",
                id, real_path, entry
            );
            return Ok(RefreshResult::Delete);
        }
        // inplace modification.
        // update metadata
        self.id_to_path.get_mut(&id).unwrap().fsmeta = meta;
        debug!(
            "Reloading entry {:?}: {:?}. Ent: {:?}",
            id, real_path, entry
        );
        Ok(RefreshResult::Reload)
    }

    pub async fn refresh_dir_list(&mut self, id: fileid3) -> Result<(), nfsstat3> {
        let entry = self
            .id_to_path
            .get(&id)
            .ok_or(nfsstat3::NFS3ERR_NOENT)?
            .clone();
        // if there are children and the metadata did not change
        if entry.children.is_some() && !fattr3_differ(&entry.children_meta, &entry.fsmeta) {
            return Ok(());
        }
        if !matches!(entry.fsmeta.ftype, ftype3::NF3DIR) {
            return Ok(());
        }

        let mut cur_path = entry.name.clone();
        let mut new_children: Vec<u64> = Vec::new();
        debug!("Relisting entry {:?}: {:?}. Ent: {:?}", id, cur_path, entry);

        // Handle root directory differently - list mount points
        if entry.name.is_empty() {
            // Root directory - list mount points
            let mounts = self.mounts.clone();
            for (target_path, source_path, _read_only) in &mounts {
                let target_sym = self
                    .intern
                    .intern(OsStr::new(target_path.trim_start_matches('/')).to_os_string())
                    .unwrap();
                cur_path.push(target_sym);

                if source_path.exists() {
                    let meta = fs::symlink_metadata(source_path)
                        .await
                        .unwrap_or_else(|_| std::fs::metadata(".").unwrap());
                    let next_id = self.create_entry(&cur_path, meta).await;
                    new_children.push(next_id);
                }
                cur_path.pop();
            }
        } else {
            // Regular directory - get real path and list contents
            let (real_path, _read_only) = match self.sym_to_real_path(&entry.name).await {
                Some(path) => path,
                None => return Ok(()), // Mount point without real path
            };

            if let Ok(mut listing) = fs::read_dir(&real_path).await {
                while let Some(entry) = listing
                    .next_entry()
                    .await
                    .map_err(|_| nfsstat3::NFS3ERR_IO)?
                {
                    let sym = self.intern.intern(entry.file_name()).unwrap();
                    cur_path.push(sym);
                    let meta = entry.metadata().await.unwrap();
                    let next_id = self.create_entry(&cur_path, meta).await;
                    new_children.push(next_id);
                    cur_path.pop();
                }
            }
        }

        self.id_to_path
            .get_mut(&id)
            .ok_or(nfsstat3::NFS3ERR_NOENT)?
            .children = Some(BTreeSet::from_iter(new_children.into_iter()));

        Ok(())
    }

    pub async fn create_entry(&mut self, fullpath: &Vec<Symbol>, meta: Metadata) -> fileid3 {
        let next_id = if let Some(chid) = self.path_to_id.get(fullpath) {
            if let Some(chent) = self.id_to_path.get_mut(chid) {
                chent.fsmeta = metadata_to_fattr3(*chid, &meta);
            }
            *chid
        } else {
            // path does not exist
            let next_id = self.next_fileid.fetch_add(1, Ordering::Relaxed);
            let metafattr = metadata_to_fattr3(next_id, &meta);
            let new_entry = FSEntry {
                name: fullpath.clone(),
                fsmeta: metafattr,
                children_meta: metafattr,
                children: None,
            };
            debug!("creating new entry {:?}: {:?}", next_id, meta);
            self.id_to_path.insert(next_id, new_entry);
            self.path_to_id.insert(fullpath.clone(), next_id);
            next_id
        };
        next_id
    }
}
