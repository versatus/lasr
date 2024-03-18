use std::process::Command;
fn main() {
    let output = Command::new("git")
        .args(&["rev-parse", "HEAD"])
        .output()
        .unwrap();
    let mut git_hash = String::from_utf8(output.stdout).unwrap();
    git_hash.pop(); // trailing \n
    let output = Command::new("git")
        .args(&["branch", "--show-current"])
        .output()
        .unwrap();
    let mut git_branch = String::from_utf8(output.stdout).unwrap();
    git_branch.pop(); // remove trailing \n
    println!("cargo:rustc-env=GIT_REV={}:{}", git_branch, git_hash);
}
