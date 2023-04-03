use color_eyre::Result;
use elf::endian::AnyEndian;
use elf::ElfBytes;
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct Section(pub String, pub u32);

pub fn size(path_str: &str) -> Result<Vec<Section>> {
    let path = std::path::PathBuf::from(path_str);
    let file_data = std::fs::read(path)?;
    let slice = file_data.as_slice();
    let file = ElfBytes::<AnyEndian>::minimal_parse(slice)?;

    let (shdrs_opt, strtab_opt) = file.section_headers_with_strtab()?;
    let (shdrs, strtab) = (
        shdrs_opt.expect("Should have shdrs"),
        strtab_opt.expect("Should have strtab"),
    );

    Ok(shdrs
        .iter()
        .filter(|shdr| shdr.sh_addr != 0 && shdr.sh_size != 0)
        .map(|shdr| {
            Section(
                strtab
                    .get(shdr.sh_name as usize)
                    .expect("Failed to get section name")
                    .to_owned(),
                shdr.sh_size as u32,
            )
        })
        .collect())
}
