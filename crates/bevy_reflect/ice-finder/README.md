The `../derive` dir (D:\bevywinicon\crates\bevy_reflect\derive) crate causes an ICE when running `cargo fmt` from the root of the workspace.

We want to perform a binary search to remove files from that crate until we find the minimal set of files that still cause the ICE.

We will write a rust program, `ice-finder`, that will be responsible for identifying the minimal set of files that cause the ICE.

The ice will be detected by [similar means to this script](../../../bevy-fmt-fail-finder.ps1); we will run `cargo fmt` in the correct dir and observe the output to determine if it succeeded or failed with an ICE.

ice-finder, when ran, will enumerate the files in the `derive` directory, and will create a list of the files that it wants to copy over to a temporary directory.

It will run the command in that dir and if it still happens, that means we need to prune more, and if it doesn't ice, we know we need to restore some of the pruned files.

Implement ice-finder; use the `eyre` and `color-eyre` crates for error handling.

Use absolute paths for all file operations to avoid confusion.

You MUST use cloud terrastodon user input picker tui to have the user confirm ALL operations before they are performed.

---

use the tempfile crate to create the directories where the copies will live.
you may only delete files by dropping the tempdir.

use cloud terrastodon pickertui pick_many to prompt the user to pick the files to include in the next test run.
After a test run, the next time you ask the user to pick the files, in the key of each Choice, you should include keywords like "ICE_FOUND" or "NO_ICE" to help them remember the last result and allow the user to easily fuzzy include/exclude entries.
In the ice-finder dir, you should track which canonicalized paths resulted in ice or not.