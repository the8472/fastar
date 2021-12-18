[![Version](https://img.shields.io/crates/v/fastar.svg)](https://crates.io/crates/fastar)

# fastar

A faster equivalent of  `tar -cT <(find . -type f)`, optimized for tarring many small files stored on HDDs.
  
Optimizations compared to gnu tar:

* directory traversal based on physical disk layout. see [platter-walk](https://github.com/the8472/platter-walk) crate 
* readaheads across multiple files at once to keep the drive's command queue filled. see [reapfrog](https://github.com/the8472/reapfrog) crate
* drops disk caches for files once they are read to prevent disk buffer thrashing.


Limitations compared to gnu tar:

* arguments must be directories
* only archives regular files, not symlinks or empty directories
* xattrs are not included
* sparse files are zero-filled


## Building

* indirect dependencies: libz, liblzo headers
* `cargo build --release`

## Benchmarks

```
# ffcnt . -s
files: 6680901
bytes: 245271028476

# echo 3 > /proc/sys/vm/drop_caches ; tar -c . | pv -at > /dev/null
^C0:02:45 [ 2.4MiB/s]

# echo 3 > /proc/sys/vm/drop_caches ; tar -cT <(ffcnt --ls --type f --leaf-order content .) | pv -at > /dev/null
^C0:02:50 [4.11MiB/s]

# echo 3 > /proc/sys/vm/drop_caches ; fastar . | pv -at > /dev/null
^C0:02:51 [9.28MiB/s]
```