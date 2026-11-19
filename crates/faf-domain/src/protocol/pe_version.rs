//! The product name in a Windows executable's version resource: how a
//! launcher tells faf-re's `ForgedAlliance.exe` from the original game's
//! (faf-re issue #314, `docs/LAUNCHER_NATIVE_NETWORK.md`).
//!
//! In FAF both files are called `ForgedAlliance.exe`, and faf-re's x86 build is
//! 32-bit like the original, so neither the name nor the machine type tells
//! them apart. The product name does: `faf-re` against `Supreme Commander
//! Forged Alliance`. It is read where Windows' `VerQueryValue` reads
//! `\StringFileInfo\040904b0\ProductName`, but out of the bytes, because the
//! game also runs under Wine on Linux, where there is no such call to make.
//!
//! Only what that path needs is read: the PE headers locate the resource
//! section, its tree locates the version resource (`RT_VERSION`), and that
//! resource's blocks locate the string table and the value. In two steps, so a
//! caller holding a file reads the headers and then the resource section and
//! nothing else: [`resource_section`], then [`product_name_in`].
//! [`product_name`] does both over an image held whole.

/// faf-re's product name. The original game's is `Supreme Commander Forged
/// Alliance`.
pub const REBUILT_ENGINE_PRODUCT: &str = "faf-re";

/// How much of the start of an image [`resource_section`] may need. The
/// headers of any real executable end well inside it.
pub const HEADER_BYTES: usize = 64 * 1024;

/// The largest resource section read. The game's carries an icon and a few
/// dialogs, well under a megabyte; a header claiming more is damage.
const MAX_SECTION_BYTES: u32 = 32 * 1024 * 1024;

/// The resource type of a version resource.
const RT_VERSION: u32 = 16;

/// U.S. English, Unicode: the string table the contract reads.
const STRING_TABLE: &str = "040904b0";

/// Where an image keeps its resources.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ResourceSection {
    /// Where its bytes start in the file.
    pub file_offset: u64,
    /// How many bytes of it to read.
    pub size: u32,
    /// Its address once loaded: the tree's data entries name addresses.
    pub rva: u32,
}

/// Locate the resource section, from the start of an image: up to
/// [`HEADER_BYTES`] of it, fewer when the file is shorter.
///
/// `None` for anything that is not a PE image with resources.
pub fn resource_section(headers: &[u8]) -> Option<ResourceSection> {
    if headers.get(..2)? != b"MZ" {
        return None;
    }
    let pe = u32_at(headers, 0x3c)? as usize;
    if headers.get(pe..pe.checked_add(4)?)? != b"PE\0\0" {
        return None;
    }
    let sections = u16_at(headers, pe + 6)? as usize;
    let optional_size = u16_at(headers, pe + 20)? as usize;
    let optional = pe + 24;
    // The data directories sit further into the 64-bit header.
    let (count_at, directories) = match u16_at(headers, optional)? {
        0x10b => (optional + 92, optional + 96),
        0x20b => (optional + 108, optional + 112),
        _ => return None,
    };
    // The third directory is the resources'.
    if u32_at(headers, count_at)? < 3 || directories + 24 > optional + optional_size {
        return None;
    }
    let rva = u32_at(headers, directories + 16)?;
    let size = u32_at(headers, directories + 20)?;
    if rva == 0 || size == 0 {
        return None;
    }

    let table = optional + optional_size;
    (0..sections).find_map(|index| {
        let header = table + index * 40;
        let virtual_size = u32_at(headers, header + 8)?;
        let address = u32_at(headers, header + 12)?;
        let raw_size = u32_at(headers, header + 16)?;
        let raw_offset = u32_at(headers, header + 20)?;
        let into = rva.checked_sub(address)?;
        if into >= virtual_size.max(raw_size) {
            return None;
        }
        // Only what the file holds: the rest of a section is zeroes once
        // loaded, and has no version resource in it.
        let size = size.min(raw_size.checked_sub(into)?);
        (size > 0 && size <= MAX_SECTION_BYTES).then_some(ResourceSection {
            file_offset: u64::from(raw_offset) + u64::from(into),
            size,
            rva,
        })
    })
}

/// The product name in a resource section's bytes, from the U.S. English,
/// Unicode string table (`040904b0`).
pub fn product_name_in(section: &ResourceSection, bytes: &[u8]) -> Option<String> {
    // Type, then name, then language: the first of each, as there is one
    // version resource and one language for it in practice.
    let names = subdirectory(bytes, 0, |id| id == RT_VERSION)?;
    let languages = subdirectory(bytes, names, |_| true)?;
    let entry = directory_entries(bytes, languages)?
        .find_map(|(_, target)| (target & 0x8000_0000 == 0).then_some(target as usize))?;
    let data = u32_at(bytes, entry)?.checked_sub(section.rva)? as usize;
    let length = u32_at(bytes, entry + 4)? as usize;
    let version = bytes.get(data..data.checked_add(length)?)?;

    let root = Block::read(version, 0)?;
    if root.key != "VS_VERSION_INFO" {
        return None;
    }
    let strings = root
        .children(version)
        .find(|child| child.key.eq_ignore_ascii_case("StringFileInfo"))?;
    let table = strings
        .children(version)
        .find(|child| child.key.eq_ignore_ascii_case(STRING_TABLE))?;
    let product = table
        .children(version)
        .find(|child| child.key.eq_ignore_ascii_case("ProductName"))?;
    utf16_until_nul(version, product.value, product.end).map(|(text, _)| text)
}

/// [`resource_section`] and [`product_name_in`] over a whole image.
pub fn product_name(image: &[u8]) -> Option<String> {
    let section = resource_section(&image[..image.len().min(HEADER_BYTES)])?;
    let start = usize::try_from(section.file_offset).ok()?;
    let bytes = image.get(start..start.checked_add(section.size as usize)?)?;
    product_name_in(&section, bytes)
}

/// A resource directory's entries: each one's name or id, and where it
/// points.
fn directory_entries(bytes: &[u8], at: usize) -> Option<impl Iterator<Item = (u32, u32)> + '_> {
    let named = u16_at(bytes, at + 12)? as usize;
    let ids = u16_at(bytes, at + 14)? as usize;
    let first = at + 16;
    Some((0..named + ids).map_while(move |index| {
        let entry = first + index * 8;
        Some((u32_at(bytes, entry)?, u32_at(bytes, entry + 4)?))
    }))
}

/// The subdirectory of the first entry at `at` whose id `wanted` takes.
fn subdirectory(bytes: &[u8], at: usize, wanted: impl Fn(u32) -> bool) -> Option<usize> {
    directory_entries(bytes, at)?.find_map(|(name, target)| {
        // A name with its top bit set is a string, never a resource type id.
        let id = (name & 0x8000_0000 == 0).then_some(name)?;
        (wanted(id) && target & 0x8000_0000 != 0).then_some((target & 0x7fff_ffff) as usize)
    })
}

/// One block of a version resource: its length, its value's length and kind,
/// its key, then its value and its children, each on a 32-bit boundary from
/// the resource's start.
struct Block {
    key: String,
    /// Where the value starts.
    value: usize,
    /// Where the children start.
    children: usize,
    /// Where the block ends.
    end: usize,
}

impl Block {
    fn read(version: &[u8], at: usize) -> Option<Self> {
        let length = u16_at(version, at)? as usize;
        let value_length = u16_at(version, at + 2)? as usize;
        let text = u16_at(version, at + 4)? == 1;
        let end = at.checked_add(length)?;
        if length < 6 || end > version.len() {
            return None;
        }
        let (key, after_key) = utf16_until_nul(version, at + 6, end)?;
        let value = align4(after_key);
        // A text value's length counts UTF-16 units, a binary one's bytes.
        let value_bytes = if text { value_length * 2 } else { value_length };
        Some(Self {
            key,
            value,
            children: align4(value + value_bytes),
            end,
        })
    }

    fn children<'a>(&self, version: &'a [u8]) -> impl Iterator<Item = Block> + 'a {
        let end = self.end;
        let mut at = self.children;
        std::iter::from_fn(move || {
            if at >= end {
                return None;
            }
            let child = Block::read(version, at)?;
            // A child reaching past its parent is damage: stop there.
            if child.end > end {
                return None;
            }
            at = align4(child.end);
            Some(child)
        })
    }
}

/// UTF-16 text from `at` to its terminating NUL, or to `end` when there is
/// none, and where the reading stopped.
fn utf16_until_nul(bytes: &[u8], at: usize, end: usize) -> Option<(String, usize)> {
    let mut units = Vec::new();
    let mut position = at;
    while position + 2 <= end {
        let unit = u16_at(bytes, position)?;
        position += 2;
        if unit == 0 {
            break;
        }
        units.push(unit);
    }
    String::from_utf16(&units).ok().map(|text| (text, position))
}

fn align4(offset: usize) -> usize {
    (offset + 3) & !3
}

fn u16_at(bytes: &[u8], at: usize) -> Option<u16> {
    let pair = bytes.get(at..at.checked_add(2)?)?;
    Some(u16::from_le_bytes([pair[0], pair[1]]))
}

fn u32_at(bytes: &[u8], at: usize) -> Option<u32> {
    let quad = bytes.get(at..at.checked_add(4)?)?;
    Some(u32::from_le_bytes([quad[0], quad[1], quad[2], quad[3]]))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The original game's product name.
    const ORIGINAL: &str = "Supreme Commander Forged Alliance";

    /// Where the fixture loads its one section, and where the file has it.
    const SECTION_RVA: u32 = 0x1000;
    const SECTION_FILE_OFFSET: u32 = 0x200;

    fn utf16z(text: &str) -> Vec<u8> {
        text.encode_utf16()
            .chain([0])
            .flat_map(u16::to_le_bytes)
            .collect()
    }

    fn pad4(bytes: &mut Vec<u8>) {
        bytes.resize(align4(bytes.len()), 0);
    }

    /// One version block, as a resource compiler writes it: the length
    /// covers the block without the padding after it.
    fn block(
        key: &str,
        text: bool,
        value: &[u8],
        value_length: u16,
        children: &[Vec<u8>],
    ) -> Vec<u8> {
        let mut out = vec![0, 0];
        out.extend(value_length.to_le_bytes());
        out.extend(u16::from(text).to_le_bytes());
        out.extend(utf16z(key));
        pad4(&mut out);
        out.extend(value);
        for child in children {
            pad4(&mut out);
            out.extend(child);
        }
        let length = out.len() as u16;
        out[..2].copy_from_slice(&length.to_le_bytes());
        out
    }

    fn string(key: &str, value: &str) -> Vec<u8> {
        let units = value.encode_utf16().count() as u16 + 1;
        block(key, true, &utf16z(value), units, &[])
    }

    /// A `VS_VERSIONINFO` with a `VarFileInfo` first, as some resource
    /// compilers order it, and one string table per entry of `tables`.
    fn version_info(tables: &[(&str, &[(&str, &str)])]) -> Vec<u8> {
        let mut fixed = vec![0; 52];
        fixed[..4].copy_from_slice(&0xfeef_04bd_u32.to_le_bytes());
        let translation = block("Translation", false, &[0x09, 0x04, 0xb0, 0x04], 4, &[]);
        let var_info = block("VarFileInfo", true, &[], 0, &[translation]);
        let tables: Vec<Vec<u8>> = tables
            .iter()
            .map(|(key, strings)| {
                let strings: Vec<Vec<u8>> = strings
                    .iter()
                    .map(|(key, value)| string(key, value))
                    .collect();
                block(key, true, &[], 0, &strings)
            })
            .collect();
        let string_info = block("StringFileInfo", true, &[], 0, &tables);
        block(
            "VS_VERSION_INFO",
            false,
            &fixed,
            52,
            &[var_info, string_info],
        )
    }

    /// A resource section holding `version` as the one version resource:
    /// type 16, name 1, language 0x409.
    fn resources(version: &[u8]) -> Vec<u8> {
        let directory = |id: u32, target: u32| {
            let mut out = vec![0; 14];
            out.extend(1_u16.to_le_bytes());
            out.extend(id.to_le_bytes());
            out.extend(target.to_le_bytes());
            out
        };
        let mut out = directory(RT_VERSION, 0x8000_0000 | 24);
        out.extend(directory(1, 0x8000_0000 | 48));
        out.extend(directory(0x409, 72));
        out.extend((SECTION_RVA + 88).to_le_bytes());
        out.extend((version.len() as u32).to_le_bytes());
        out.extend([0; 8]);
        assert_eq!(out.len(), 88);
        out.extend(version);
        out
    }

    /// A PE image with one `.rsrc` section holding `version`: 32-bit or
    /// 64-bit, as the game's builds are. No code, but whole enough for
    /// Windows' own `GetFileVersionInfo` to read, which is how the fixture
    /// files under `fixtures/` were checked against the system's reading.
    fn image(version: &[u8], wide: bool) -> Vec<u8> {
        let section = resources(version);
        let size = section.len() as u32;
        let raw_size = size.next_multiple_of(SECTION_FILE_OFFSET);
        let optional_size: u16 = if wide { 240 } else { 224 };
        let mut out = vec![0; SECTION_FILE_OFFSET as usize];
        let mut put = |at: usize, bytes: &[u8]| out[at..at + bytes.len()].copy_from_slice(bytes);
        put(0, b"MZ");
        put(0x3c, &0x80_u32.to_le_bytes());
        put(0x80, b"PE\0\0");
        let (machine, characteristics): (u16, u16) = if wide {
            (0x8664, 0x0022)
        } else {
            (0x014c, 0x0102)
        };
        put(0x84, &machine.to_le_bytes());
        put(0x86, &1_u16.to_le_bytes());
        put(0x94, &optional_size.to_le_bytes());
        put(0x96, &characteristics.to_le_bytes());
        let optional = 0x98;
        let (magic, count_at, directories) = if wide {
            put(optional + 24, &0x1_4000_0000_u64.to_le_bytes());
            (0x20b_u16, optional + 108, optional + 112)
        } else {
            put(optional + 28, &0x40_0000_u32.to_le_bytes());
            (0x10b_u16, optional + 92, optional + 96)
        };
        put(optional, &magic.to_le_bytes());
        put(optional + 32, &SECTION_RVA.to_le_bytes()); // section alignment
        put(optional + 36, &SECTION_FILE_OFFSET.to_le_bytes()); // file alignment
        put(optional + 48, &6_u16.to_le_bytes()); // subsystem version
        put(
            optional + 56,
            &(SECTION_RVA + size.next_multiple_of(SECTION_RVA)).to_le_bytes(),
        ); // image size
        put(optional + 60, &SECTION_FILE_OFFSET.to_le_bytes()); // header size
        put(optional + 68, &2_u16.to_le_bytes()); // a windowed program
        put(count_at, &16_u32.to_le_bytes());
        put(directories + 16, &SECTION_RVA.to_le_bytes());
        put(directories + 20, &size.to_le_bytes());
        let header = optional + usize::from(optional_size);
        put(header, b".rsrc\0\0\0");
        put(header + 8, &size.to_le_bytes());
        put(header + 12, &SECTION_RVA.to_le_bytes());
        put(header + 16, &raw_size.to_le_bytes());
        put(header + 20, &SECTION_FILE_OFFSET.to_le_bytes());
        put(header + 36, &0x4000_0040_u32.to_le_bytes()); // initialised, readable
        out.extend(section);
        out.resize((SECTION_FILE_OFFSET + raw_size) as usize, 0);
        out
    }

    /// The executable as faf-re's or the original game's: their own versions,
    /// as `MohoVersion.rc` and the retail resource give them.
    fn named(product: &str) -> Vec<u8> {
        let version = if product == REBUILT_ENGINE_PRODUCT {
            "2026.11.001"
        } else {
            "1, 5, 0, 1"
        };
        image(
            &version_info(&[(
                "040904b0",
                &[
                    ("FileDescription", "Forged Alliance"),
                    ("FileVersion", version),
                    ("ProductName", product),
                ],
            )]),
            false,
        )
    }

    /// The images under `fixtures/` are this builder's, so the launcher's
    /// reading of a file is tested on bytes accounted for field by field
    /// here. Windows' own `FileVersionInfo` reads the same names out of them.
    #[test]
    fn the_fixture_files_are_the_builders_images() {
        assert_eq!(
            include_bytes!("fixtures/pe-faf-re.bin").as_slice(),
            named(REBUILT_ENGINE_PRODUCT)
        );
        assert_eq!(
            include_bytes!("fixtures/pe-original.bin").as_slice(),
            named(ORIGINAL)
        );
    }

    #[test]
    fn faf_res_exe_is_told_from_the_original() {
        assert_eq!(
            product_name(&named(REBUILT_ENGINE_PRODUCT)).as_deref(),
            Some("faf-re")
        );
        assert_eq!(product_name(&named(ORIGINAL)).as_deref(), Some(ORIGINAL));
    }

    #[test]
    fn a_64_bit_image_reads_the_same() {
        let version = version_info(&[("040904b0", &[("ProductName", "faf-re")])]);
        assert_eq!(
            product_name(&image(&version, true)).as_deref(),
            Some("faf-re")
        );
    }

    #[test]
    fn the_table_key_is_matched_in_either_case() {
        // rc.exe writes the key as the .rc file spells it, `040904B0` as often.
        let version = version_info(&[("040904B0", &[("ProductName", "faf-re")])]);
        assert_eq!(
            product_name(&image(&version, false)).as_deref(),
            Some("faf-re")
        );
    }

    #[test]
    fn only_the_contracts_string_table_counts() {
        let version = version_info(&[
            ("040704b0", &[("ProductName", "faf-re")]),
            ("040904b0", &[("ProductName", ORIGINAL)]),
        ]);
        assert_eq!(
            product_name(&image(&version, false)).as_deref(),
            Some(ORIGINAL)
        );
        let german_only = version_info(&[("040704b0", &[("ProductName", "faf-re")])]);
        assert_eq!(product_name(&image(&german_only, false)), None);
    }

    #[test]
    fn the_headers_and_then_the_section_are_all_that_is_read() {
        // How a launcher reads a file: the headers first, then the bytes they
        // point at, never the whole executable.
        let image = named("faf-re");
        let section = resource_section(&image[..SECTION_FILE_OFFSET as usize]).unwrap();
        assert_eq!(u64::from(SECTION_FILE_OFFSET), section.file_offset);
        let start = section.file_offset as usize;
        let bytes = &image[start..start + section.size as usize];
        assert_eq!(product_name_in(&section, bytes).as_deref(), Some("faf-re"));
    }

    #[test]
    fn anything_that_is_not_such_an_image_has_no_name() {
        assert_eq!(product_name(b""), None);
        assert_eq!(product_name(b"#!/bin/sh\necho hello\n"), None);
        // An ELF binary, which is what faf-re's Linux build is.
        assert_eq!(product_name(b"\x7fELF\x02\x01\x01\0\0\0\0\0\0\0\0\0"), None);

        // Cut anywhere, a valid image never panics, and is never read as the
        // whole thing.
        let image = named("faf-re");
        for cut in 0..image.len() - 1 {
            if let Some(name) = product_name(&image[..cut]) {
                assert!("faf-re".starts_with(&name), "cut at {cut}: {name:?}");
            }
        }
    }

    #[test]
    fn a_header_pointing_outside_the_image_is_refused() {
        let mut image = named("faf-re");
        // The section's file offset, far past the end.
        let header = 0x98 + 224;
        image[header + 20..header + 24].copy_from_slice(&0x7fff_0000_u32.to_le_bytes());
        assert_eq!(product_name(&image), None);

        // A version resource whose data entry names an address before the
        // section.
        let mut image = named("faf-re");
        let entry = SECTION_FILE_OFFSET as usize + 72;
        image[entry..entry + 4].copy_from_slice(&0x10_u32.to_le_bytes());
        assert_eq!(product_name(&image), None);
    }
}
