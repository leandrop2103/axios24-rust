fn main() {
    if std::env::var_os("CARGO_CFG_WINDOWS").is_some() {
        let mut res = winresource::WindowsResource::new();
        res.set_icon("assets/ferris_mixer.ico");
        res.set("ProductName", "Duonn Axios 24 DS Mixer Pro");
        res.set("FileDescription", "Duonn Axios 24 Digital Mixer Control Software");
        if let Err(e) = res.compile() {
            println!("cargo:warning=winresource compile failed: {}", e);
        }
    }
}
