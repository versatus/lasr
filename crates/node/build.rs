use std::process::Command;
fn main() {
    let git_hash_output = Command::new("git").args(&["rev-parse", "HEAD"]).output();
    let git_branch_output = Command::new("git")
        .args(&["branch", "--show-current"])
        .output();

    if let (Ok(hash_output), Ok(branch_output)) = (git_hash_output, git_branch_output) {
        if let (Ok(mut git_hash), Ok(mut git_branch)) = (
            String::from_utf8(hash_output.stdout),
            String::from_utf8(branch_output.stdout),
        ) {
            git_hash.pop(); // trailing \n
            git_branch.pop(); // remove trailing \n
            println!("cargo:rustc-env=GIT_REV={}:{}", git_branch, git_hash);
        }
    }
}
