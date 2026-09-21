use std::{env, fs, path::PathBuf};

fn make_icon(path: &std::path::Path) -> std::io::Result<()> {
    const W: usize = 64;
    const H: usize = 64;
    let mut rgba = vec![[0u8; 4]; W * H];

    let mut set = |x: usize, y: usize, c: [u8; 4]| {
        if x < W && y < H { rgba[y * W + x] = c; }
    };

    let dark = [38, 48, 56, 255];
    let metal = [168, 184, 194, 255];
    let light = [222, 232, 238, 255];
    let accent = [36, 176, 142, 255];

    for y in 18..55 {
        for x in 16..48 {
            if x == 16 || x == 47 || y == 54 { set(x, y, dark); }
            else { set(x, y, metal); }
        }
    }
    for y in 14..19 { for x in 12..52 { set(x, y, dark); } }
    for y in 10..14 { for x in 24..40 { set(x, y, dark); } }
    for y in 22..49 {
        for &x in &[24usize, 32, 40] { for dx in 0..2 { set(x + dx, y, light); } }
    }
    for y in 55..59 { for x in 20..44 { set(x, y, accent); } }

    let mask_stride = ((W + 31) / 32) * 4;
    let pixel_bytes = W * H * 4;
    let mask_bytes = mask_stride * H;
    let dib_size = 40 + pixel_bytes + mask_bytes;

    let mut out = Vec::with_capacity(6 + 16 + dib_size);
    out.extend_from_slice(&0u16.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes());

    out.push(W as u8);
    out.push(H as u8);
    out.push(0);
    out.push(0);
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&32u16.to_le_bytes());
    out.extend_from_slice(&(dib_size as u32).to_le_bytes());
    out.extend_from_slice(&(22u32).to_le_bytes());

    out.extend_from_slice(&40u32.to_le_bytes());
    out.extend_from_slice(&(W as i32).to_le_bytes());
    out.extend_from_slice(&((H * 2) as i32).to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&32u16.to_le_bytes());
    out.extend_from_slice(&0u32.to_le_bytes());
    out.extend_from_slice(&(pixel_bytes as u32).to_le_bytes());
    out.extend_from_slice(&0i32.to_le_bytes());
    out.extend_from_slice(&0i32.to_le_bytes());
    out.extend_from_slice(&0u32.to_le_bytes());
    out.extend_from_slice(&0u32.to_le_bytes());

    for y in (0..H).rev() {
        for x in 0..W {
            let [r, g, b, a] = rgba[y * W + x];
            out.extend_from_slice(&[b, g, r, a]);
        }
    }
    out.resize(out.len() + mask_bytes, 0);
    fs::write(path, out)
}

fn main() {
    #[cfg(windows)]
    {
        let out = PathBuf::from(env::var("OUT_DIR").unwrap());
        let icon = out.join("apocalipse-faxina.ico");
        make_icon(&icon).expect("falha ao gerar icone");
        let mut res = winresource::WindowsResource::new();
        res.set_icon(icon.to_str().unwrap());
        res.set("FileDescription", "Apocalipse Faxina");
        res.set("ProductName", "Apocalipse Faxina");
        res.set("CompanyName", "linuxhell");
        res.set("LegalCopyright", "linuxhell");
        res.set_manifest(r#"
<assembly xmlns="urn:schemas-microsoft-com:asm.v1" manifestVersion="1.0">
  <trustInfo xmlns="urn:schemas-microsoft-com:asm.v3">
    <security>
      <requestedPrivileges>
        <requestedExecutionLevel level="requireAdministrator" uiAccess="false" />
      </requestedPrivileges>
    </security>
  </trustInfo>
  <compatibility xmlns="urn:schemas-microsoft-com:compatibility.v1">
    <application>
      <supportedOS Id="{8e0f7a12-bfb3-4fe8-b9a5-48fd50a15a9a}"/>
      <supportedOS Id="{4f476546-937c-4f91-bd50-8c82b245de47}"/>
    </application>
  </compatibility>
</assembly>
"#);
        res.compile().expect("falha ao compilar recursos do Windows");
    }
}
