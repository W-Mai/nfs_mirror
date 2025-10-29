mod cli;
mod config;
mod daemon;

use std::collections::{BTreeSet, HashMap};
use std::ffi::{OsStr, OsString};
use std::fs::Metadata;
use std::io::SeekFrom;
use std::ops::Bound;
use std::os::unix::ffi::OsStrExt;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use async_trait::async_trait;
use clap::Parser;
use intaglio::Symbol;
use intaglio::osstr::SymbolTable;
use tokio::fs::{File, OpenOptions};
use tokio::io::{AsyncReadExt, AsyncSeekExt, AsyncWriteExt};
use tracing::debug;
use tracing_subscriber::FmtSubscriber;

use zerofs_nfsserve::fs_util::*;
use zerofs_nfsserve::nfs::*;
use zerofs_nfsserve::tcp::{NFSTcp, NFSTcpListener};
use zerofs_nfsserve::vfs::{AuthContext, DirEntry, NFSFileSystem, ReadDirResult, VFSCapabilities};

use cli::Cli;
use daemon::{change_working_directory, handle_daemon_mode};

#[derive(Debug, Clone)]
struct FSEntry {
    name: Vec<Symbol>,
    fsmeta: fattr3,
    /// metadata when building the children list
    children_meta: fattr3,
    children: Option<BTreeSet<fileid3>>,
}

/// File system mapping structure
#[derive(Debug)]
struct FSMap {
    /// Mount configurations
    mounts: Vec<(String, PathBuf, bool)>, // (target_path, source_path, read_only)
    /// Next file ID counter
    next_fileid: AtomicU64,
    /// Symbol table for interned strings
    intern: SymbolTable,
    /// Mapping from file ID to file system entry
    id_to_path: HashMap<fileid3, FSEntry>,
    /// Mapping from path symbols to file ID
    path_to_id: HashMap<Vec<Symbol>, fileid3>,
}

enum RefreshResult {
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
    fn new_with_root(root_dir: PathBuf) -> FSMap {
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
    fn new_with_mounts(root_dir: PathBuf, mounts: Vec<(String, PathBuf, bool)>) -> FSMap {
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

    // fn new(mounts: Vec<(String, PathBuf, bool)>) -> FSMap {
    //     // For backward compatibility, use current directory as root
    //     Self::new_with_mounts(PathBuf::from("."), mounts)
    // }

    /// Get the actual file system path for a given symbolic path
    async fn sym_to_real_path(&self, symlist: &[Symbol]) -> Option<(PathBuf, bool)> {
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

    async fn sym_to_path(&self, symlist: &[Symbol]) -> PathBuf {
        let mut ret = PathBuf::new();
        for i in symlist.iter() {
            ret.push(self.intern.get(*i).unwrap());
        }
        ret
    }

    async fn sym_to_fname(&self, symlist: &[Symbol]) -> OsString {
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

    fn delete_entry(&mut self, id: fileid3) {
        let mut children = Vec::new();
        self.collect_all_children(id, &mut children);
        for i in children.iter() {
            if let Some(ent) = self.id_to_path.remove(i) {
                self.path_to_id.remove(&ent.name);
            }
        }
    }

    fn find_entry(&self, id: fileid3) -> Result<FSEntry, nfsstat3> {
        Ok(self
            .id_to_path
            .get(&id)
            .ok_or(nfsstat3::NFS3ERR_NOENT)?
            .clone())
    }
    fn find_entry_mut(&mut self, id: fileid3) -> Result<&mut FSEntry, nfsstat3> {
        self.id_to_path.get_mut(&id).ok_or(nfsstat3::NFS3ERR_NOENT)
    }
    async fn find_child(&self, id: fileid3, filename: &[u8]) -> Result<fileid3, nfsstat3> {
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
    async fn refresh_entry(&mut self, id: fileid3) -> Result<RefreshResult, nfsstat3> {
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
                                let meta = tokio::fs::symlink_metadata(source_path)
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

        let meta = tokio::fs::symlink_metadata(&real_path)
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
    async fn refresh_dir_list(&mut self, id: fileid3) -> Result<(), nfsstat3> {
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
                    let meta = tokio::fs::symlink_metadata(source_path)
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

            if let Ok(mut listing) = tokio::fs::read_dir(&real_path).await {
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

    async fn create_entry(&mut self, fullpath: &Vec<Symbol>, meta: Metadata) -> fileid3 {
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
/// Mirror file system implementation
#[derive(Debug)]
pub struct MirrorFS {
    /// File system mapping protected by mutex
    fsmap: tokio::sync::Mutex<FSMap>,
    /// Read-only mode flag
    read_only: bool,
}

/// Enumeration for the create_fs_object method
enum CreateFSObject {
    /// Creates a directory
    Directory,
    /// Creates a file with a set of attributes
    File(sattr3),
    /// Creates an exclusive file with a set of attributes
    Exclusive,
    /// Creates a symlink with a set of attributes to a target location
    Symlink((sattr3, nfspath3)),
}
impl MirrorFS {
    /// Create a new mirror file system with root directory only
    pub fn new(root_dir: PathBuf, read_only: bool) -> MirrorFS {
        MirrorFS {
            fsmap: tokio::sync::Mutex::new(FSMap::new_with_root(root_dir)),
            read_only,
        }
    }

    /// Create a new mirror file system with mount points
    pub fn new_with_mounts(
        root_dir: PathBuf,
        read_only: bool,
        mounts: Vec<config::MountConfig>,
    ) -> MirrorFS {
        // Convert MountConfig to (String, PathBuf, bool) format
        let mount_tuples: Vec<(String, PathBuf, bool)> = mounts
            .into_iter()
            .map(|m| (m.target, m.source, m.read_only))
            .collect();

        MirrorFS {
            fsmap: tokio::sync::Mutex::new(FSMap::new_with_mounts(root_dir, mount_tuples)),
            read_only,
        }
    }

    /// creates a FS object in a given directory and of a given type
    async fn create_fs_object(
        &self,
        dirid: fileid3,
        objectname: &filename3,
        object: &CreateFSObject,
    ) -> Result<(fileid3, fattr3), nfsstat3> {
        if self.read_only {
            return Err(nfsstat3::NFS3ERR_ROFS);
        }

        let mut fsmap = self.fsmap.lock().await;
        let ent = fsmap.find_entry(dirid)?;

        // Get the real file system path for the directory
        let (dir_path, dir_read_only) = match fsmap.sym_to_real_path(&ent.name).await {
            Some(path) => path,
            None => {
                // This is a mount point, cannot create objects here
                return Err(nfsstat3::NFS3ERR_ACCES);
            }
        };

        if dir_read_only {
            return Err(nfsstat3::NFS3ERR_ROFS);
        }

        let mut path = dir_path;
        let objectname_osstr = OsStr::from_bytes(objectname).to_os_string();
        path.push(&objectname_osstr);

        match object {
            CreateFSObject::Directory => {
                debug!("mkdir {:?}", path);
                if exists_no_traverse(&path) {
                    return Err(nfsstat3::NFS3ERR_EXIST);
                }
                tokio::fs::create_dir(&path)
                    .await
                    .map_err(|_| nfsstat3::NFS3ERR_IO)?;
            }
            CreateFSObject::File(setattr) => {
                debug!("create {:?}", path);
                let file = std::fs::File::create(&path).map_err(|_| nfsstat3::NFS3ERR_IO)?;
                let _ = file_setattr(&file, setattr).await;
            }
            CreateFSObject::Exclusive => {
                debug!("create exclusive {:?}", path);
                let _ = std::fs::File::options()
                    .write(true)
                    .create_new(true)
                    .open(&path)
                    .map_err(|_| nfsstat3::NFS3ERR_EXIST)?;
            }
            CreateFSObject::Symlink((_, target)) => {
                debug!("symlink {:?} {:?}", path, target);
                if exists_no_traverse(&path) {
                    return Err(nfsstat3::NFS3ERR_EXIST);
                }
                tokio::fs::symlink(OsStr::from_bytes(target), &path)
                    .await
                    .map_err(|_| nfsstat3::NFS3ERR_IO)?;
                // we do not set attributes on symlinks
            }
        }

        let _ = fsmap.refresh_entry(dirid).await;

        let sym = fsmap.intern.intern(objectname_osstr).unwrap();
        let mut name = ent.name.clone();
        name.push(sym);
        let meta = path.symlink_metadata().map_err(|_| nfsstat3::NFS3ERR_IO)?;
        let fileid = fsmap.create_entry(&name, meta.clone()).await;

        // update the children list
        if let Some(ref mut children) = fsmap
            .id_to_path
            .get_mut(&dirid)
            .ok_or(nfsstat3::NFS3ERR_NOENT)?
            .children
        {
            children.insert(fileid);
        }
        Ok((fileid, metadata_to_fattr3(fileid, &meta)))
    }
}

#[async_trait]
impl NFSFileSystem for MirrorFS {
    fn root_dir(&self) -> fileid3 {
        0
    }
    fn capabilities(&self) -> VFSCapabilities {
        if self.read_only {
            VFSCapabilities::ReadOnly
        } else {
            VFSCapabilities::ReadWrite
        }
    }

    async fn lookup(
        &self,
        _auth: &AuthContext,
        dirid: fileid3,
        filename: &filename3,
    ) -> Result<fileid3, nfsstat3> {
        let mut fsmap = self.fsmap.lock().await;
        if let Ok(id) = fsmap.find_child(dirid, filename).await {
            if fsmap.id_to_path.contains_key(&id) {
                return Ok(id);
            }
        }
        // Optimize for negative lookups.
        // See if the file actually exists on the filesystem
        let dirent = fsmap.find_entry(dirid)?;

        // Get the real file system path for the directory
        let (dir_path, _dir_read_only) = match fsmap.sym_to_real_path(&dirent.name).await {
            Some(path) => path,
            None => {
                // This is a mount point, check if it's the mount point itself
                if dirent.name.len() == 1 {
                    let mount_name = fsmap
                        .intern
                        .get(dirent.name[0])
                        .ok_or(nfsstat3::NFS3ERR_NOENT)?;
                    for (target_path, _source_path, _) in &fsmap.mounts {
                        if mount_name == OsStr::new(target_path.trim_start_matches('/')) {
                            // Check if the filename matches this mount point
                            let filename_str = OsStr::from_bytes(filename);
                            if filename_str == mount_name {
                                // This is a lookup for the mount point itself
                                return Ok(dirid);
                            }
                        }
                    }
                }
                return Err(nfsstat3::NFS3ERR_NOENT);
            }
        };

        let mut path = dir_path;
        let objectname_osstr = OsStr::from_bytes(filename).to_os_string();
        path.push(&objectname_osstr);
        if !exists_no_traverse(&path) {
            return Err(nfsstat3::NFS3ERR_NOENT);
        }
        // ok the file actually exists.
        // that means something changed under me probably.
        // refresh.

        if let RefreshResult::Delete = fsmap.refresh_entry(dirid).await? {
            return Err(nfsstat3::NFS3ERR_NOENT);
        }
        let _ = fsmap.refresh_dir_list(dirid).await;

        fsmap.find_child(dirid, filename).await
        //debug!("lookup({:?}, {:?})", dirid, filename);

        //debug!(" -- lookup result {:?}", res);
    }

    async fn getattr(&self, _auth: &AuthContext, id: fileid3) -> Result<fattr3, nfsstat3> {
        //debug!("Stat query {:?}", id);
        let mut fsmap = self.fsmap.lock().await;
        if let RefreshResult::Delete = fsmap.refresh_entry(id).await? {
            return Err(nfsstat3::NFS3ERR_NOENT);
        }
        let ent = fsmap.find_entry(id)?;
        let path = fsmap.sym_to_path(&ent.name).await;
        debug!("Stat {:?}: {:?}", path, ent);
        Ok(ent.fsmeta)
    }

    async fn read(
        &self,
        _auth: &AuthContext,
        id: fileid3,
        offset: u64,
        count: u32,
    ) -> Result<(Vec<u8>, bool), nfsstat3> {
        let fsmap = self.fsmap.lock().await;
        let ent = fsmap.find_entry(id)?;

        // Get the real file system path
        let (path, _read_only) = match fsmap.sym_to_real_path(&ent.name).await {
            Some(path) => path,
            None => {
                // This is a mount point or root, cannot read
                return Err(nfsstat3::NFS3ERR_ISDIR);
            }
        };

        drop(fsmap);
        let mut f = File::open(&path).await.or(Err(nfsstat3::NFS3ERR_NOENT))?;
        let len = f.metadata().await.or(Err(nfsstat3::NFS3ERR_NOENT))?.len();
        let mut start = offset;
        let mut end = offset + count as u64;
        let eof = end >= len;
        if start >= len {
            start = len;
        }
        if end > len {
            end = len;
        }
        f.seek(SeekFrom::Start(start))
            .await
            .or(Err(nfsstat3::NFS3ERR_IO))?;
        let mut buf = vec![0; (end - start) as usize];
        f.read_exact(&mut buf).await.or(Err(nfsstat3::NFS3ERR_IO))?;
        Ok((buf, eof))
    }

    async fn readdir(
        &self,
        _auth: &AuthContext,
        dirid: fileid3,
        start_after: fileid3,
        max_entries: usize,
    ) -> Result<ReadDirResult, nfsstat3> {
        let mut fsmap = self.fsmap.lock().await;
        fsmap.refresh_entry(dirid).await?;
        fsmap.refresh_dir_list(dirid).await?;

        let entry = fsmap.find_entry(dirid)?;
        if !matches!(entry.fsmeta.ftype, ftype3::NF3DIR) {
            return Err(nfsstat3::NFS3ERR_NOTDIR);
        }
        debug!("readdir({:?}, {:?})", entry, start_after);
        // we must have children here
        let children = entry.children.ok_or(nfsstat3::NFS3ERR_IO)?;

        let mut ret = ReadDirResult {
            entries: Vec::new(),
            end: false,
        };

        let range_start = if start_after > 0 {
            Bound::Excluded(start_after)
        } else {
            Bound::Unbounded
        };

        let remaining_length = children.range((range_start, Bound::Unbounded)).count();
        let path = fsmap.sym_to_path(&entry.name).await;
        debug!("path: {:?}", path);
        debug!("children len: {:?}", children.len());
        debug!("remaining_len : {:?}", remaining_length);
        for i in children.range((range_start, Bound::Unbounded)) {
            let fileid = *i;
            let fileent = fsmap.find_entry(fileid)?;
            let name = fsmap.sym_to_fname(&fileent.name).await;
            debug!("\t --- {:?} {:?}", fileid, name);
            ret.entries.push(DirEntry {
                fileid,
                name: name.as_bytes().into(),
                attr: fileent.fsmeta,
            });
            if ret.entries.len() >= max_entries {
                break;
            }
        }
        if ret.entries.len() == remaining_length {
            ret.end = true;
        }
        debug!("readdir_result:{:?}", ret);

        Ok(ret)
    }

    async fn setattr(
        &self,
        _auth: &AuthContext,
        id: fileid3,
        setattr: sattr3,
    ) -> Result<fattr3, nfsstat3> {
        let mut fsmap = self.fsmap.lock().await;
        let entry = fsmap.find_entry(id)?;
        let path = fsmap.sym_to_path(&entry.name).await;
        path_setattr(&path, &setattr).await?;

        // I have to lookup a second time to update
        let metadata = path.symlink_metadata().or(Err(nfsstat3::NFS3ERR_IO))?;
        if let Ok(entry) = fsmap.find_entry_mut(id) {
            entry.fsmeta = metadata_to_fattr3(id, &metadata);
        }
        Ok(metadata_to_fattr3(id, &metadata))
    }
    async fn write(
        &self,
        _auth: &AuthContext,
        id: fileid3,
        offset: u64,
        data: &[u8],
    ) -> Result<fattr3, nfsstat3> {
        if self.read_only {
            return Err(nfsstat3::NFS3ERR_ROFS);
        }
        let fsmap = self.fsmap.lock().await;
        let ent = fsmap.find_entry(id)?;

        // Get the real file system path
        let (path, read_only) = match fsmap.sym_to_real_path(&ent.name).await {
            Some(path) => path,
            None => {
                // This is a mount point or root, cannot write
                return Err(nfsstat3::NFS3ERR_ISDIR);
            }
        };

        if read_only {
            return Err(nfsstat3::NFS3ERR_ROFS);
        }

        drop(fsmap);
        debug!("write to init {:?}", path);
        let mut f = OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(false)
            .open(&path)
            .await
            .map_err(|e| {
                debug!("Unable to open {:?}", e);
                nfsstat3::NFS3ERR_IO
            })?;
        f.seek(SeekFrom::Start(offset)).await.map_err(|e| {
            debug!("Unable to seek {:?}", e);
            nfsstat3::NFS3ERR_IO
        })?;
        f.write_all(data).await.map_err(|e| {
            debug!("Unable to write {:?}", e);
            nfsstat3::NFS3ERR_IO
        })?;
        debug!("write to {:?} {:?} {:?}", path, offset, data.len());
        let _ = f.flush().await;
        let _ = f.sync_all().await;
        let meta = f.metadata().await.or(Err(nfsstat3::NFS3ERR_IO))?;
        Ok(metadata_to_fattr3(id, &meta))
    }

    async fn create(
        &self,
        _auth: &AuthContext,
        dirid: fileid3,
        filename: &filename3,
        setattr: sattr3,
    ) -> Result<(fileid3, fattr3), nfsstat3> {
        self.create_fs_object(dirid, filename, &CreateFSObject::File(setattr))
            .await
    }

    async fn create_exclusive(
        &self,
        _auth: &AuthContext,
        dirid: fileid3,
        filename: &filename3,
    ) -> Result<fileid3, nfsstat3> {
        Ok(self
            .create_fs_object(dirid, filename, &CreateFSObject::Exclusive)
            .await?
            .0)
    }

    async fn remove(
        &self,
        _auth: &AuthContext,
        dirid: fileid3,
        filename: &filename3,
    ) -> Result<(), nfsstat3> {
        if self.read_only {
            return Err(nfsstat3::NFS3ERR_ROFS);
        }

        let mut fsmap = self.fsmap.lock().await;
        let ent = fsmap.find_entry(dirid)?;

        // Get the real file system path for the directory
        let (dir_path, dir_read_only) = match fsmap.sym_to_real_path(&ent.name).await {
            Some(path) => path,
            None => {
                // This is a mount point, cannot remove objects here
                return Err(nfsstat3::NFS3ERR_ACCES);
            }
        };

        if dir_read_only {
            return Err(nfsstat3::NFS3ERR_ROFS);
        }

        let mut path = dir_path;
        path.push(OsStr::from_bytes(filename));

        if let Ok(meta) = path.symlink_metadata() {
            if meta.is_dir() {
                tokio::fs::remove_dir(&path)
                    .await
                    .map_err(|_| nfsstat3::NFS3ERR_IO)?;
            } else {
                tokio::fs::remove_file(&path)
                    .await
                    .map_err(|_| nfsstat3::NFS3ERR_IO)?;
            }

            let filesym = fsmap
                .intern
                .intern(OsStr::from_bytes(filename).to_os_string())
                .unwrap();
            let mut sympath = ent.name.clone();
            sympath.push(filesym);
            if let Some(fileid) = fsmap.path_to_id.get(&sympath).copied() {
                // update the fileid -> path
                // and the path -> fileid mappings for the deleted file
                fsmap.id_to_path.remove(&fileid);
                fsmap.path_to_id.remove(&sympath);
                // we need to update the children listing for the directories
                if let Ok(dirent_mut) = fsmap.find_entry_mut(dirid) {
                    if let Some(ref mut fromch) = dirent_mut.children {
                        fromch.remove(&fileid);
                    }
                }
            }

            let _ = fsmap.refresh_entry(dirid).await;
        } else {
            return Err(nfsstat3::NFS3ERR_NOENT);
        }

        Ok(())
    }

    async fn rename(
        &self,
        _auth: &AuthContext,
        from_dirid: fileid3,
        from_filename: &filename3,
        to_dirid: fileid3,
        to_filename: &filename3,
    ) -> Result<(), nfsstat3> {
        if self.read_only {
            return Err(nfsstat3::NFS3ERR_ROFS);
        }

        let mut fsmap = self.fsmap.lock().await;

        let from_dirent = fsmap.find_entry(from_dirid)?;
        let (from_dir_path, from_read_only) = match fsmap.sym_to_real_path(&from_dirent.name).await
        {
            Some(path) => path,
            None => {
                // This is a mount point, cannot rename from here
                return Err(nfsstat3::NFS3ERR_ACCES);
            }
        };

        let to_dirent = fsmap.find_entry(to_dirid)?;
        let (to_dir_path, to_read_only) = match fsmap.sym_to_real_path(&to_dirent.name).await {
            Some(path) => path,
            None => {
                // This is a mount point, cannot rename to here
                return Err(nfsstat3::NFS3ERR_ACCES);
            }
        };

        if from_read_only || to_read_only {
            return Err(nfsstat3::NFS3ERR_ROFS);
        }

        let mut from_path = from_dir_path;
        from_path.push(OsStr::from_bytes(from_filename));

        let mut to_path = to_dir_path;
        // to folder must exist
        if !exists_no_traverse(&to_path) {
            return Err(nfsstat3::NFS3ERR_NOENT);
        }
        to_path.push(OsStr::from_bytes(to_filename));

        // src path must exist
        if !exists_no_traverse(&from_path) {
            return Err(nfsstat3::NFS3ERR_NOENT);
        }
        debug!("Rename {:?} to {:?}", from_path, to_path);
        tokio::fs::rename(&from_path, &to_path)
            .await
            .map_err(|_| nfsstat3::NFS3ERR_IO)?;

        let oldsym = fsmap
            .intern
            .intern(OsStr::from_bytes(from_filename).to_os_string())
            .unwrap();
        let newsym = fsmap
            .intern
            .intern(OsStr::from_bytes(to_filename).to_os_string())
            .unwrap();

        let mut from_sympath = from_dirent.name.clone();
        from_sympath.push(oldsym);
        let mut to_sympath = to_dirent.name.clone();
        to_sympath.push(newsym);
        if let Some(fileid) = fsmap.path_to_id.get(&from_sympath).copied() {
            // update the fileid -> path
            // and the path -> fileid mappings for the new file
            fsmap.id_to_path.get_mut(&fileid).unwrap().name = to_sympath.clone();
            fsmap.path_to_id.remove(&from_sympath);
            fsmap.path_to_id.insert(to_sympath, fileid);
            if to_dirid != from_dirid {
                // moving across directories.
                // we need to update the children listing for the directories
                if let Ok(from_dirent_mut) = fsmap.find_entry_mut(from_dirid) {
                    if let Some(ref mut fromch) = from_dirent_mut.children {
                        fromch.remove(&fileid);
                    }
                }
                if let Ok(to_dirent_mut) = fsmap.find_entry_mut(to_dirid) {
                    if let Some(ref mut toch) = to_dirent_mut.children {
                        toch.insert(fileid);
                    }
                }
            }
        }
        let _ = fsmap.refresh_entry(from_dirid).await;
        if to_dirid != from_dirid {
            let _ = fsmap.refresh_entry(to_dirid).await;
        }

        Ok(())
    }
    async fn mkdir(
        &self,
        _auth: &AuthContext,
        dirid: fileid3,
        dirname: &filename3,
        _attrs: &sattr3,
    ) -> Result<(fileid3, fattr3), nfsstat3> {
        self.create_fs_object(dirid, dirname, &CreateFSObject::Directory)
            .await
    }

    async fn symlink(
        &self,
        _auth: &AuthContext,
        dirid: fileid3,
        linkname: &filename3,
        symlink: &nfspath3,
        attr: &sattr3,
    ) -> Result<(fileid3, fattr3), nfsstat3> {
        self.create_fs_object(
            dirid,
            linkname,
            &CreateFSObject::Symlink((*attr, symlink.clone())),
        )
        .await
    }
    async fn readlink(&self, _auth: &AuthContext, id: fileid3) -> Result<nfspath3, nfsstat3> {
        let fsmap = self.fsmap.lock().await;
        let ent = fsmap.find_entry(id)?;

        // Get the real file system path
        let (path, _read_only) = match fsmap.sym_to_real_path(&ent.name).await {
            Some(path) => path,
            None => {
                // This is a mount point or root, cannot readlink
                return Err(nfsstat3::NFS3ERR_BADTYPE);
            }
        };

        drop(fsmap);
        if path.is_symlink() {
            if let Ok(target) = path.read_link() {
                Ok(target.as_os_str().as_bytes().into())
            } else {
                Err(nfsstat3::NFS3ERR_IO)
            }
        } else {
            Err(nfsstat3::NFS3ERR_BADTYPE)
        }
    }

    async fn mknod(
        &self,
        _auth: &AuthContext,
        dirid: fileid3,
        filename: &filename3,
        ftype: ftype3,
        attr: &sattr3,
        spec: Option<&specdata3>,
    ) -> Result<(fileid3, fattr3), nfsstat3> {
        // For mirrorfs, we'll create regular files for special file types
        // since creating actual device files requires elevated privileges
        match ftype {
            ftype3::NF3CHR | ftype3::NF3BLK => {
                // Create a regular file to represent the device
                // In a real implementation, you would use spec.specdata1 (major) and spec.specdata2 (minor)
                if let Some(_device_spec) = spec {
                    // Could log or store device major/minor info here
                }
                self.create_fs_object(dirid, filename, &CreateFSObject::File(*attr))
                    .await
            }
            ftype3::NF3SOCK | ftype3::NF3FIFO => {
                // FIFOs can be created with mkfifo, but for simplicity create regular files
                self.create_fs_object(dirid, filename, &CreateFSObject::File(*attr))
                    .await
            }
            _ => Err(nfsstat3::NFS3ERR_BADTYPE),
        }
    }

    async fn link(
        &self,
        _auth: &AuthContext,
        fileid: fileid3,
        linkdirid: fileid3,
        linkname: &filename3,
    ) -> Result<(), nfsstat3> {
        if self.read_only {
            return Err(nfsstat3::NFS3ERR_ROFS);
        }

        let mut fsmap = self.fsmap.lock().await;

        // Get the file path
        let file_entry = fsmap.find_entry(fileid)?;
        let (file_path, _file_read_only) = match fsmap.sym_to_real_path(&file_entry.name).await {
            Some(path) => path,
            None => {
                // This is a mount point or root, cannot link
                return Err(nfsstat3::NFS3ERR_ACCES);
            }
        };

        // Get the link directory path
        let linkdir_entry = fsmap.find_entry(linkdirid)?;
        let (link_dir_path, link_read_only) =
            match fsmap.sym_to_real_path(&linkdir_entry.name).await {
                Some(path) => path,
                None => {
                    // This is a mount point, cannot create link here
                    return Err(nfsstat3::NFS3ERR_ACCES);
                }
            };

        if link_read_only {
            return Err(nfsstat3::NFS3ERR_ROFS);
        }

        let mut link_path = link_dir_path;
        link_path.push(OsStr::from_bytes(linkname));

        // Create the hard link
        tokio::fs::hard_link(&file_path, &link_path)
            .await
            .map_err(|e| {
                debug!("Failed to create hard link: {:?}", e);
                match e.kind() {
                    std::io::ErrorKind::PermissionDenied => nfsstat3::NFS3ERR_ACCES,
                    std::io::ErrorKind::NotFound => nfsstat3::NFS3ERR_NOENT,
                    std::io::ErrorKind::AlreadyExists => nfsstat3::NFS3ERR_EXIST,
                    _ => nfsstat3::NFS3ERR_IO,
                }
            })?;

        // Update the fsmap with the new link
        let link_sym = fsmap
            .intern
            .intern(OsStr::from_bytes(linkname).to_os_string())
            .unwrap();
        let mut link_sympath = linkdir_entry.name.clone();
        link_sympath.push(link_sym);

        // The link points to the same fileid as the original file
        fsmap.path_to_id.insert(link_sympath.clone(), fileid);

        // Update the directory's children if needed
        if let Ok(linkdir_entry_mut) = fsmap.find_entry_mut(linkdirid) {
            if let Some(ref mut children) = linkdir_entry_mut.children {
                children.insert(fileid);
            }
        }

        Ok(())
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Parse command line arguments
    let cli = Cli::parse();

    // Initialize logging
    let subscriber = FmtSubscriber::builder()
        .with_max_level(cli.get_log_level())
        .with_ansi(!cli.no_color)
        .finish();
    tracing::subscriber::set_global_default(subscriber)?;

    // Load configuration
    let config = cli.load_config()?;

    // Handle daemon mode
    if config.server.daemon {
        handle_daemon_mode(&cli)?;
    }

    // Change working directory if specified
    change_working_directory(&config.server.work_dir)?;

    // Parse allowed IP addresses
    let allowed_ips = cli.parse_allowed_ips();

    // Print startup information
    Cli::print_startup_info(&config, &allowed_ips);

    // Create NFS file system - use the first mount's source as root directory
    let root_dir = if !config.mounts.is_empty() {
        config.mounts[0].source.canonicalize()?
    } else {
        return Err("No mount points configured".into());
    };

    let fs = MirrorFS::new_with_mounts(root_dir, config.server.read_only, config.mounts);

    // Start NFS TCP server
    let addr = format!("{}:{}", config.server.ip, config.server.port).parse()?;
    let listener = NFSTcpListener::bind(addr, fs).await?;

    // Start the server
    listener.handle_forever().await?;

    Ok(())
}
