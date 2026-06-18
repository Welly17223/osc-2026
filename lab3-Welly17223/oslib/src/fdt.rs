use core::ffi::c_str::CStr;
use crate::uart::Uart;

#[repr(C)]
struct FdtHeader {
    magic: u32,
    totalsize: u32,
    off_dt_struct: u32,
    off_dt_strings: u32,
    off_mem_rsvmap: u32,
    version: u32,
    last_comp_version: u32,
    boot_cpuid_phys: u32,
    size_dt_strings: u32,
    size_dt_struct: u32,
}

#[derive(Clone, Copy)]
#[repr(C)]
pub struct FdtProp {
    len: u32,
    name_off: u32,
}

pub enum TokenContent {
    BeginNode {
        name: &'static str,
    },
    EndNode,
    End,
    Nop,
    Prop {
        name: &'static str,
        prop_header: &'static FdtProp,
        prop: *const u8,
    },
}

pub struct FdtIter {
    fdt_block_address: *const u8,
    fdt_str_address: *const u8,
    fdt_iter_depth: usize,
}

impl FdtIter {
    pub fn new(block_addr: *const u8, str_addr: *const u8) -> Self {
        FdtIter {
            fdt_block_address: block_addr,
            fdt_str_address: str_addr,
            fdt_iter_depth: 0,
        }
    }

    pub fn from_fdt(fdt: *const u8) -> Option<FdtIter> {
        let fdt_header = fdt as *const FdtHeader;
        if fdt.is_null() {
            return None;
        }
        if unsafe { (*fdt_header).magic }.swap_bytes() != FDT_MAGIC {
            return None;
        }

        let fdt_block_address =
            fdt.wrapping_byte_offset(unsafe { (*fdt_header).off_dt_struct }.swap_bytes() as isize);
        let fdt_str_address =
            fdt.wrapping_byte_offset(unsafe { (*fdt_header).off_dt_strings }.swap_bytes() as isize);

        Some(FdtIter {
            fdt_block_address,
            fdt_str_address,
            fdt_iter_depth: 0,
        })
    }
}

impl Iterator for FdtIter {
    type Item = (TokenContent, usize);
    fn next(&mut self) -> Option<Self::Item> {
        let token_ptr = self.fdt_block_address as *const u32;
        self.fdt_block_address = self.fdt_block_address.wrapping_byte_offset(4);

        match unsafe { *token_ptr }.swap_bytes() {
            // BeginNode
            0x1 => {
                self.fdt_iter_depth += 1;
                let node_name = unsafe { CStr::from_ptr(token_ptr.wrapping_add(1) as *const u8) }
                    .to_str()
                    .ok()?;

                self.fdt_block_address = self
                    .fdt_block_address
                    .wrapping_byte_offset(node_name.len() as isize + 1);
                self.fdt_block_address = align(self.fdt_block_address as u64, 4) as *const u8;

                Some((
                    TokenContent::BeginNode { name: node_name },
                    self.fdt_iter_depth - 1,
                ))
            }
            // EndNode
            0x2 => {
                self.fdt_iter_depth -= 1;
                Some((TokenContent::EndNode, self.fdt_iter_depth))
            }
            // Prop
            0x3 => {
                let fdt_prop = token_ptr.wrapping_add(1) as *const FdtProp;

                self.fdt_block_address = self.fdt_block_address.wrapping_byte_offset(
                    (unsafe { (*fdt_prop).len }.swap_bytes() as usize + size_of::<FdtProp>())
                        as isize,
                );
                self.fdt_block_address = align(self.fdt_block_address as u64, 4) as *const u8;

                let prop_name = unsafe {
                    CStr::from_ptr(
                        self.fdt_str_address
                            .wrapping_byte_offset((*fdt_prop).name_off.swap_bytes() as isize),
                    )
                }
                .to_str()
                .ok()?;

                Some((
                    TokenContent::Prop {
                        name: prop_name,
                        prop_header: unsafe { fdt_prop.as_ref()? },
                        prop: fdt_prop.wrapping_offset(1) as *const u8,
                    },
                    self.fdt_iter_depth,
                ))
            }
            // Nop
            0x4 => Some((TokenContent::Nop, self.fdt_iter_depth)),
            // End
            0x9 => None,
            _ => None,
        }
    }
}

const FDT_MAGIC: u32 = 0xd00dfeed;

#[inline]
fn align(n: u64, byte: u32) -> u64 {
    let mask = (byte - 1) as u64;
    (n + mask) & !mask
}

enum Token {
    BeginNode,
    EndNode,
    End,
    Nop,
    Prop,
}

#[derive(Debug)]
pub enum Error {
    Notfound,
    NullPtr,
    NotFDT(u32),
    NotTok(u32),
    Utf8Error,
}

impl From<Token> for u32 {
    fn from(val: Token) -> Self {
        match val {
            Token::BeginNode => 0x1,
            Token::EndNode => 0x2,
            Token::End => 0x9,
            Token::Nop => 0x4,
            Token::Prop => 0x3,
        }
    }
}

impl TryFrom<u32> for Token {
    type Error = u32;
    fn try_from(value: u32) -> Result<Self, u32> {
        match value {
            1 => Ok(Token::BeginNode),
            2 => Ok(Token::EndNode),
            3 => Ok(Token::Prop),
            4 => Ok(Token::Nop),
            9 => Ok(Token::End),
            v => Err(v),
        }
    }
}

pub fn path_all_offset(
    fdt: *const u8,
    path: &str,
    offset_arr: &mut [usize],
) -> Result<usize, Error> {
    use Error::*;
    let fdt_iter = FdtIter::from_fdt(fdt).ok_or(NotFDT(0))?;

    let mut split_str: [&str; 32] = [""; 32];
    let mut path_depth = 0usize;
    for (i, sub_path) in path.split_terminator("/").take(32).enumerate() {
        let sub_path = if let Some(ind) = sub_path.find('@') {
            &sub_path[0..ind]
        } else {
            sub_path
        };
        split_str[i] = sub_path;
        path_depth += 1;
    }

    let mut match_depth = 0usize;
    let mut match_num = 0usize;

    for (i, (token, current_depth)) in fdt_iter.enumerate() {
        match token {
            TokenContent::BeginNode { name: node_name } => {
                let node_name = if let Some(ind) = node_name.find("@") {
                    &node_name[0..ind]
                } else {
                    node_name
                };

                if current_depth < split_str.len()
                    && current_depth - match_depth == 1
                    && (node_name == split_str[current_depth] || split_str[current_depth] == "*")
                {
                    match_depth += 1;
                }

                if match_depth == path_depth - 1 {
                    // match_depth -= 1;
                    offset_arr[match_num] = i;
                    match_num += 1;
                }

                if match_num >= offset_arr.len() {
                    return Ok(match_num);
                }
            }
            TokenContent::EndNode => {
                match_depth = match_depth.saturating_sub(1);
            }
            _ => {
                continue;
            }
        };
    }

    if match_num == 0 {
        Err(Notfound)
    } else {
        Ok(match_num)
    }
}

pub fn path_offset(fdt: *const u8, path: &str, order: i32) -> Result<usize, Error> {
    let fdt_iter = FdtIter::from_fdt(fdt).ok_or(Error::NotFDT(0))?;

    let mut split_str: [&str; 32] = [""; 32];
    let mut path_depth = 0usize;
    for (i, sub_path) in path.split_terminator("/").take(32).enumerate() {
        let sub_path = if let Some(ind) = sub_path.find('@') {
            &sub_path[0..ind]
        } else {
            sub_path
        };
        split_str[i] = sub_path;
        path_depth += 1;
    }

    let mut match_depth = 0usize;
    let mut match_order = 0_i32;

    for (i, (token, current_depth)) in fdt_iter.enumerate() {
        if let TokenContent::BeginNode { name: node_name } = token {
            let node_name = if let Some(ind) = node_name.find("@") {
                &node_name[0..ind]
            } else {
                node_name
            };

            if current_depth < split_str.len()
                && current_depth - match_depth == 1
                && node_name == split_str[current_depth]
            {
                match_depth += 1;
            }

            if match_depth == path_depth - 1 {
                match_depth = 0;
                match_order += 1;
            }

            if match_order == order {
                return Ok(i);
            }
        }
    }
    Err(Error::Notfound)
}

pub fn getprop(fdt: *const u8, nth: usize, name: &str) -> Result<(*mut u8, usize), Error> {
    let mut fdt_iter = FdtIter::from_fdt(fdt).ok_or(Error::NotFDT(0))?;
    let (_, node_depth) = fdt_iter.nth(nth).unwrap();

    for (token, depth) in fdt_iter {
        let (prop_name, prop_header, prop) = match token {
            TokenContent::Prop {
                name,
                prop_header,
                prop,
            } => (name, prop_header, prop),
            TokenContent::EndNode if depth == node_depth => return Err(Error::Notfound),
            _ => continue,
        };

        if prop_name == name {
            return Ok((prop as *mut u8, prop_header.len.swap_bytes() as usize));
        }
    }

    Err(Error::Notfound)
}

pub fn dump_tree(uart_dev: &Uart, fdt: *const u8) {
    let iter = if let Some(f) = FdtIter::from_fdt(fdt) {
        f
    } else {
        uart_dev.puts("Decode error\n");
        return;
    };
    let indent = |depth| {
        for _ in 0..(depth * 4) {
            uart_dev.putc(b' ');
        }
    };
    for (tok, dep) in iter {
        match tok {
            TokenContent::BeginNode { name } => {
                indent(dep);
                uart_dev.puts(name);
                uart_dev.puts(":\n");
            }
            TokenContent::Prop {
                name,
                prop_header: prop,
                prop: _,
            } => {
                indent(dep);
                uart_dev.puts(name);
                uart_dev.puts(": ");
                uart_dev.put_dec(prop.len.swap_bytes() as u64);
                uart_dev.puts("\n");
            }
            _ => (),
        }
    }
}
