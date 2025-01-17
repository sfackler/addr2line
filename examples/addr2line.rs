extern crate addr2line;
extern crate clap;
extern crate fallible_iterator;
extern crate gimli;
extern crate memmap;
extern crate object;

use std::path::Path;
use std::io::{BufRead, Lines, StdinLock};
use std::borrow::Cow;

use clap::{App, Arg, Values};
use fallible_iterator::FallibleIterator;

use addr2line::{Context, FullContext, Location};

fn parse_uint_from_hex_string(string: &str) -> u64 {
    if string.len() > 2 && string.starts_with("0x") {
        u64::from_str_radix(&string[2..], 16).expect("Failed to parse address")
    } else {
        u64::from_str_radix(string, 16).expect("Failed to parse address")
    }
}

enum VarCon<R: gimli::Reader> {
    Light(Context<R>),
    Full(FullContext<R>),
}

enum Addrs<'a> {
    Args(Values<'a>),
    Stdin(Lines<StdinLock<'a>>),
}

impl<'a> Iterator for Addrs<'a> {
    type Item = u64;

    fn next(&mut self) -> Option<u64> {
        let text = match *self {
            Addrs::Args(ref mut vals) => vals.next().map(Cow::from),
            Addrs::Stdin(ref mut lines) => lines.next().map(Result::unwrap).map(Cow::from),
        };
        text.as_ref()
            .map(Cow::as_ref)
            .map(parse_uint_from_hex_string)
    }
}

fn print_loc(loc: &Option<Location>, basenames: bool, llvm: bool) {
    if let Some(ref loc) = *loc {
        let file = loc.file.as_ref().unwrap();
        let path = if basenames {
            Path::new(file.file_name().unwrap())
        } else {
            file
        };
        print!("{}:", path.display());
        if llvm {
            print!("{}:{}", loc.line.unwrap_or(0), loc.column.unwrap_or(0));
        } else if let Some(line) = loc.line {
            print!("{}", line);
        } else {
            print!("?");
        }
        println!();
    } else if llvm {
        println!("??:0:0");
    } else {
        println!("??:0");
    }
}

fn main() {
    let matches = App::new("hardliner")
        .version("0.1")
        .about("A fast addr2line clone")
        .arg(
            Arg::with_name("exe")
                .short("e")
                .long("exe")
                .value_name("filename")
                .help(
                    "Specify the name of the executable for which addresses should be translated.",
                )
                .required(true),
        )
        .arg(
            Arg::with_name("functions")
                .short("f")
                .long("functions")
                .help("Display function names as well as file and line number information."),
        )
        .arg(
            Arg::with_name("pretty")
                .short("p")
                .long("pretty-print")
                .help(
                    "Make the output more human friendly: each location are printed on \
                     one line.",
                ),
        )
        .arg(Arg::with_name("inlines").short("i").long("inlines").help(
            "If the address belongs to a function that was inlined, the source \
             information for all enclosing scopes back to the first non-inlined \
             function will also be printed.",
        ))
        .arg(
            Arg::with_name("addresses")
                .short("a")
                .long("addresses")
                .help(
                    "Display the address before the function name, file and line \
                     number information.",
                ),
        )
        .arg(
            Arg::with_name("basenames")
                .short("s")
                .long("basenames")
                .help("Display only the base of each file name."),
        )
        .arg(Arg::with_name("demangle").short("C").long("demangle").help(
            "Demangle function names. \
             Specifying a specific demangling style (like GNU addr2line) \
             is not supported. (TODO)",
        ))
        .arg(
            Arg::with_name("llvm")
                .long("llvm")
                .help("Display output in the same format as llvm-symbolizer."),
        )
        .arg(
            Arg::with_name("addrs")
                .takes_value(true)
                .multiple(true)
                .help("Addresses to use instead of reading from stdin."),
        )
        .get_matches();

    let do_functions = matches.is_present("functions");
    let do_inlines = matches.is_present("inlines");
    let pretty = matches.is_present("pretty");
    let print_addrs = matches.is_present("addresses");
    let basenames = matches.is_present("basenames");
    let demangle = matches.is_present("demangle");
    let llvm = matches.is_present("llvm");
    let path = matches.value_of("exe").unwrap();

    let map = memmap::Mmap::open_path(path, memmap::Protection::Read).unwrap();
    let file = &object::File::parse(unsafe { map.as_slice() }).unwrap();

    let ctx = Context::new(file).unwrap();

    let ctx = if do_functions || do_inlines {
        VarCon::Full(ctx.parse_functions().unwrap())
    } else {
        VarCon::Light(ctx)
    };

    let stdin = std::io::stdin();
    let addrs = matches
        .values_of("addrs")
        .map(Addrs::Args)
        .unwrap_or_else(|| Addrs::Stdin(stdin.lock().lines()));

    for probe in addrs {
        if print_addrs {
            if llvm {
                print!("0x{:x}", probe);
            } else {
                print!("0x{:016x}", probe);
            }
            if pretty {
                print!(": ");
            } else {
                println!();
            }
        }

        match ctx {
            VarCon::Light(ref ctx) => {
                let loc = ctx.find_location(probe).unwrap();
                print_loc(&loc, basenames, llvm);
            }
            VarCon::Full(ref ctx) => {
                let mut printed_anything = false;
                let mut frames = ctx.query(probe).unwrap().enumerate();
                while let Some((i, frame)) = frames.next().unwrap() {
                    if pretty && i != 0 {
                        print!(" (inlined by) ");
                    }

                    if do_functions {
                        if let Some(func) = frame.function {
                            if demangle {
                                print!("{}", func);
                            } else {
                                print!("{}", func.raw_name().unwrap());
                            }
                        } else {
                            print!("??");
                        }

                        if pretty {
                            print!(" at ");
                        } else {
                            println!();
                        }
                    }

                    print_loc(&frame.location, basenames, llvm);

                    printed_anything = true;

                    if !do_inlines {
                        break;
                    }
                }

                if !printed_anything {
                    if do_functions {
                        print!("??");
                        if pretty {
                            print!(" ");
                        } else {
                            println!();
                        }
                    }

                    if llvm {
                        println!("??:0:0");
                    } else {
                        println!("??:0");
                    }
                }
            }
        }

        if llvm {
            println!();
        }
    }
}
