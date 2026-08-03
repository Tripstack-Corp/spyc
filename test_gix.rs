fn main() {
    let repo = gix::open("/Users/drek/src/spyc.worktrees/agy-mcp-support").unwrap();
    let common_dir = std::fs::canonicalize(repo.common_dir()).unwrap();
    let main_repo = gix::open(&common_dir).unwrap();
    println!("{:?}", main_repo.work_dir().unwrap());
}
