const WINDOWS_ICON: &str = "../../assets/icon/icon.ico";
const PRODUCT_NAME: &str = "Disk Analyzer";

fn building_for_windows() -> bool {
    std::env::var("CARGO_CFG_TARGET_OS").is_ok_and(|os| os == "windows")
}

fn main() {
    println!("cargo::rerun-if-changed={WINDOWS_ICON}");
    if !building_for_windows() {
        return;
    }
    let mut resource = winresource::WindowsResource::new();
    resource.set_icon(WINDOWS_ICON).set("ProductName", PRODUCT_NAME).set("FileDescription", PRODUCT_NAME);
    resource.compile().expect("could not embed the Windows icon and version info");
}
