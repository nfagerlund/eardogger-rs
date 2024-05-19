use std::fs::File;
use std::io::Write;
use std::process::Command;

fn main() {
    // trigger recompilation when a new migration is added
    println!("cargo:rerun-if-changed=migrations");
    // write the current git revision to VERSION.txt
    println!("cargo:rerun-if-changed=.git/HEAD");
    let git_out = Command::new("git")
        .args(["rev-parse", "HEAD"])
        .output()
        .expect("failed to execute git rev-parse HEAD");
    let date_out = Command::new("date")
        .output()
        .expect("failed to execute date");
    let mut f = File::create("./VERSION.txt").expect("couldn't open VERSION.txt file for write");
    f.write_all(&git_out.stdout).expect("couldn't write sha");
    f.write_all(&date_out.stdout).expect("couldn't write date");
}
