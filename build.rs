fn main() {
    println!("cargo:rerun-if-changed=ui");
    slint_build::compile("ui/shell.slint").expect("failed to compile Slint UI definition");
}
