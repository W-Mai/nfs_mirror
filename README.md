# NFS Mirror

This application mirrors a local directory into an NFS shared service.

The original code is from [nfsserve](https://github.com/xetdata/nfsserve).

# Usage

```
nfs_mirror /path/to/mirror
```

# Mount the NFS share

```
mount -t nfs 127.0.0.1:/path/to/mirror /mnt/mirror
mount -t nfs -o async,nolocks,rsize=1048576,wsize=1048576,tcp,port=11111,mountport=11111,hard 127.0.0.1:/path/to/mirror /mnt/mirror
```
