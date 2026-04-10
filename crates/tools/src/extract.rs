use std::path::Path;

use anyhow::bail;
use fs_err as fs;
use fs_err::File;
use pex::Layout;
use zip::ZipArchive;

pub(crate) fn unzip(pex: &Path, dest_dir: &Path) -> anyhow::Result<()> {
    if !pex.is_file() {
        match Layout::load(pex) {
            Ok(layout) => bail!(
                "The PEX at {path} is a {layout} PEX which is already in extracted form.",
                path = pex.display(),
                layout = layout.as_ref()
            ),
            Err(_err) => bail!(
                "The directory at {path} does not appear to be a PEX.",
                path = pex.display()
            ),
        }
    }
    let mut pex_zip = ZipArchive::new(File::open(pex)?)?;
    fs::create_dir_all(dest_dir)?;
    pex_zip.extract(dest_dir)?;
    Ok(())
}
