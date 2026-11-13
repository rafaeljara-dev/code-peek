use std::env;
use std::fs;
use std::path::PathBuf;

const RT_ICON: u16 = 3;
const RT_GROUP_ICON: u16 = 14;

fn main() {
    if env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }
    let manifest_dir =
        PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR"));
    let ico_path = manifest_dir
        .join("..")
        .join("..")
        .join("assets")
        .join("icon.ico");
    println!("cargo:rerun-if-changed={}", ico_path.display());

    let ico = fs::read(&ico_path).expect("read assets/icon.ico");
    let res = build_win32_icon_res(&ico).expect("valid icon.ico header");

    let out_dir = PathBuf::from(env::var_os("OUT_DIR").expect("OUT_DIR"));
    let res_path = out_dir.join("codepeek_icon.res");
    fs::write(&res_path, res).expect("write Win32 resource file");
    println!("cargo:rustc-link-arg-bin=peek={}", res_path.display());
}

fn read_u16(bytes: &[u8], offset: usize) -> Option<u16> {
    Some(u16::from_le_bytes(
        bytes.get(offset..offset + 2)?.try_into().ok()?,
    ))
}

fn read_u32(bytes: &[u8], offset: usize) -> Option<u32> {
    Some(u32::from_le_bytes(
        bytes.get(offset..offset + 4)?.try_into().ok()?,
    ))
}

fn append_res_record(out: &mut Vec<u8>, type_id: u16, name_id: u16, data: &[u8]) {
    while !out.len().is_multiple_of(4) {
        out.push(0);
    }
    out.extend_from_slice(&(data.len() as u32).to_le_bytes());
    out.extend_from_slice(&32u32.to_le_bytes());
    out.extend_from_slice(&0xFFFFu16.to_le_bytes());
    out.extend_from_slice(&type_id.to_le_bytes());
    out.extend_from_slice(&0xFFFFu16.to_le_bytes());
    out.extend_from_slice(&name_id.to_le_bytes());
    out.extend_from_slice(&0u32.to_le_bytes());
    out.extend_from_slice(&0x1030u16.to_le_bytes());
    out.extend_from_slice(&0x0409u16.to_le_bytes());
    out.extend_from_slice(&0u32.to_le_bytes());
    out.extend_from_slice(&0u32.to_le_bytes());
    out.extend_from_slice(data);
}

fn build_win32_icon_res(ico: &[u8]) -> Option<Vec<u8>> {
    if read_u16(ico, 0) != Some(0) || read_u16(ico, 2) != Some(1) {
        return None;
    }
    let count = usize::from(read_u16(ico, 4)?);
    if count == 0 {
        return None;
    }

    let mut res = Vec::with_capacity(ico.len() + 256);
    // Win32 .res null resource header (32 bytes).
    res.extend_from_slice(&0u32.to_le_bytes());
    res.extend_from_slice(&32u32.to_le_bytes());
    res.extend_from_slice(&0xFFFFu16.to_le_bytes());
    res.extend_from_slice(&0x0000u16.to_le_bytes());
    res.extend_from_slice(&0xFFFFu16.to_le_bytes());
    res.extend_from_slice(&0x0000u16.to_le_bytes());
    res.extend_from_slice(&[0u8; 16]);

    let mut group = Vec::with_capacity(6 + count * 14);
    group.extend_from_slice(&0u16.to_le_bytes());
    group.extend_from_slice(&1u16.to_le_bytes());
    group.extend_from_slice(&(count as u16).to_le_bytes());

    for index in 0..count {
        let entry_off = 6 + index * 16;
        let entry = ico.get(entry_off..entry_off + 16)?;
        let size = usize::try_from(read_u32(entry, 8)?).ok()?;
        let data_off = usize::try_from(read_u32(entry, 12)?).ok()?;
        let image = ico.get(data_off..data_off + size)?;
        let id = u16::try_from(index + 1).ok()?;

        append_res_record(&mut res, RT_ICON, id, image);
        group.extend_from_slice(&entry[..12]);
        group.extend_from_slice(&id.to_le_bytes());
    }

    append_res_record(&mut res, RT_GROUP_ICON, 1, &group);
    Some(res)
}
