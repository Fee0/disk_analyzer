# To do on other systems

The size accounting work was done and tested on Windows 10 (NTFS) and on Linux
in WSL 2 (ext4, plus btrfs and XFS on loop images). These items need a machine
that this one can't stand in for. Tick them off there.

## macOS (APFS)

- [ ] Run the full checks natively: `cargo fmt --all`, `cargo clippy --workspace
      --all-targets -- -D warnings`, `cargo test --workspace`. So far macOS has
      only been cross-checked with clippy.
- [ ] Check that `getattrlistbulk` still parses correctly now that the request
      no longer includes `ATTR_FILE_TOTALSIZE`. Covered by `bulk.rs` tests
      `reads_entries_with_sizes` and
      `file_sizes_parse_correctly_alongside_clone_info`, and by the scan test
      `counts_pure_clones_once`.
- [ ] Partially edited APFS clones are still counted in full. Only pure clones
      (same `ATTR_CMNEXT_CLONEID`) count once. Fix: for files with
      `EF_MAY_SHARE_BLOCKS`, map their physical ranges with
      `fcntl(F_LOG2PHYS_EXT)` and report them as `DirEntry::shared_blocks`
      (space = the volume's device). The Linux ledger in
      `scan/shared_blocks.rs` then counts each block once, with no changes to
      core. Test: clone a 1 MB file, overwrite 16 KB of the clone, and expect
      about 1 MB + 16 KB in total (the Linux test
      `reflinked_copies_count_shared_blocks_once_…` shows the shape).
- [ ] Measure a whole-disk scan with `examples/scan.rs -- / --disk` before and
      after that change. `docs/scan-performance.md` has the baseline. Only
      files flagged `EF_MAY_SHARE_BLOCKS` should pay for the extra `fcntl`.
- [ ] Check that the list shows "(clone)" for a pure clone and
      "(partly a clone)" for an edited one.
- [ ] Check that APFS snapshots still show up as "Hidden space" in a
      whole-disk scan, and that the total matches Disk Utility.

## Windows

- [ ] ReFS block clones (Dev Drive on Windows 11, ReFS on Windows Server) are
      counted in full: since Windows 11 24H2, `CopyFile` on a Dev Drive makes
      block clones. Windows 10 Pro can't create ReFS volumes, so this needs
      Windows 11 or Server. Find out whether `FSCTL_GET_RETRIEVAL_POINTERS`
      reports the same LCNs for cloned ranges. If so, report them as
      `DirEntry::shared_blocks` (space = volume serial) and the core ledger
      does the rest. Test with `FSCTL_DUPLICATE_EXTENTS_TO_FILE` in a temp dir
      on the Dev Drive.
- [ ] Check Volume Shadow Copies (System Restore) show up as "Hidden space" in
      a whole-disk scan of `C:`, and that the total matches Explorer's
      "Used space".

## Linux (real hardware)

- [ ] Whole-disk scan of `/` on a btrfs root with snapper or Timeshift
      snapshots. Compare the total with `btrfs filesystem du -s /` (or
      `compsize`) and with `df`. Snapshots under `/.snapshots` must not be
      counted as extra data.
- [ ] Same with the whole btrfs pool also mounted (`mount -o subvolid=5 …
      /mnt/pool`): the scan must skip `/mnt/pool/@` and `/mnt/pool/@home`.
      This is covered by a mountinfo unit test, but hasn't been checked on a
      real system.
- [ ] Time a whole-disk scan of an XFS or btrfs root with many reflinked files
      against the numbers in `docs/scan-performance.md` (WSL loop images,
      not a real SSD).
- [ ] Nested btrfs subvolumes that aren't mounted (e.g. `/var/lib/machines`,
      Docker's btrfs storage) are not entered, so their space shows up as
      "Hidden space". Decide whether a whole-disk scan should enter the ones
      that aren't snapshots.
