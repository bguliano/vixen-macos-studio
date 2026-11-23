//! The tests' target folder is the one Cargo built them into, also from a worktree that builds into the bay's.
mod support;

#[test]
fn target_dir_is_the_folder_holding_this_test() {
    let target = support::target_dir();
    assert!(target.join(".rustc_info.json").is_file(), "{}", target.display());
    assert!(std::env::current_exe().unwrap().starts_with(&target));
}
