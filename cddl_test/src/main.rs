
// We need to figure how out we handle integers as they can only be serialized as
// Unsigned or Negative. Do we do an enum for the int type?
// It's only used in transaction_metadata as one choice.
// How it has to be serialized (assuming number: i32):
// if number >= 0 {
//     serializer.write_unsigned_integer(number as u64)
// } else {
//     serializer.write_negative_integer(number as i64)
// }

use cddl::ast::*;
use std::collections::BTreeMap;

//static free_groups = std::collections::BTreeMap<Group>;

struct GlobalScope {
    global_scope: codegen::Scope,
    already_generated: std::collections::BTreeSet<String>,
}

impl GlobalScope {
    fn new() -> Self {
        Self {
            global_scope: codegen::Scope::new(),
            already_generated: std::collections::BTreeSet::new(),
        }
    }

    // direct raw access
    fn scope(&mut self) -> &mut codegen::Scope {
        &mut self.global_scope
    }

    // generate array type ie [Foo] generates Foos if not already created
    fn generate_array_type(&mut self, element_type: String) {
        let array_type = format!("{}s", element_type);
        if self.already_generated.insert(array_type.clone()) {
            let mut s = codegen::Struct::new(&array_type);
            s
                .field("data", format!("Vec<{}>", element_type))
                .vis("pub")
                .derive("Clone");
            // TODO: accessors/wasm exposure
            self.global_scope.raw("#[wasm_bindgen]");
            self.global_scope.push_struct(s);
            let mut ser_impl = codegen::Impl::new(&array_type);
            ser_impl.impl_trait("cbor_event::se::Serialize");
            let mut ser_func = make_serialization_function("serialize");
            ser_func.line("serializer.write_array(cbor_event::Len::Len(self.data.len() as u64))?;");
            let mut loop_block = codegen::Block::new("for element in &self.data");
            loop_block.line("element.serialize(serializer)?;");
            ser_func.push_block(loop_block);
            ser_func.line("Ok(serializer)");
            ser_impl.push_fn(ser_func);
            self.global_scope.push_impl(ser_impl);
            let mut array_impl = codegen::Impl::new(&array_type);
            array_impl
                .new_fn("new")
                .ret("Self")
                .line("Self { data: Vec::new() }");
            array_impl
                .new_fn("size")
                .ret("usize")
                .arg_ref_self()
                .line("self.data.len()");
            array_impl
                .new_fn("get")
                .ret(&element_type)
                .arg_ref_self()
                .arg("index", "usize")
                .line("self.data[index].clone()");
            array_impl
                .new_fn("add")
                .arg_mut_self()
                .arg("elem", &element_type)
                .line("self.data.push(elem);");
            self.global_scope.raw("#[wasm_bindgen]");
            self.global_scope.push_impl(array_impl);
        }
    }
}

fn group_entry_to_field_name(entry: &GroupEntry, index: usize) -> String {
    match entry {
        GroupEntry::ValueMemberKey(vmk) => match vmk.member_key.as_ref() {
            Some(member_key) => match member_key {
                MemberKey::Value(value) => format!("key_{}", value),
                MemberKey::Bareword(ident) => ident.to_string(),
                MemberKey::Type1(_) => panic!("Encountered Type1 member key in multi-field map - not supported"),
            },
            None => format!("index_{}", index),
        },
        GroupEntry::TypeGroupname(_) => {
            // This was before, but it makes more sense for what we've done so far
            // to have it be indexed. This may or may not be correct.
            //("tgn_".to_owned() + &tge.name.to_string()),
            format!("index_{}", index)
        },
        GroupEntry::InlineGroup(_) => panic!("not implemented (define a new struct for this!)"),
    }
}

// TODO: Can we do this, or do we need to be more explicit to match the schema?
fn convert_types(raw: &str) -> &str {
    match raw {
        "uint" => "u64",
        // Not sure on this one, I think they can be bigger than i64 can fit
        // but the cbor_event serialization takes the argument as an i64
        "nint" => "i64",
        // TODO: define enum or something as otherwise it can overflow i64
        // and also we can't define the serialization traits for types
        // that are defined outside of this crate (includes primitives)
        //"int" => "i64",
        "tstr" | "text" => "String",
        // TODO: Is this right to have it be Vec<u8>?
        // the serialization library for bytes takes type [u8]
        // so we'll have to put some logic in there I guess?
        // it might be necessary to put a wrapper type..
        "bstr" | "bytes" => "Bytes",
        // What about bingint/other stuff in the standard prelude?
        x => x,
    }
}

// Returns None if this is a fixed value that we should not be storing
fn rust_type_from_type2(global: &mut GlobalScope, type2: &Type2) -> Option<String> {
    match type2 {
        // ignoring IntValue/FloatValue/other primitives since they're not in the shelley spec
        // ie Type2::UintValue(value) => format!("uint<{}>", value),
        // generic args not in shelley.cddl
        // TODO: socket plugs (used in hash type)
        Type2::Typename((ident, _generic_arg)) => Some(convert_types(&ident.ident).to_owned()),
        // Map(group) not implemented as it's not in shelley.cddl
        Type2::Array(group) => {
            let mut s = String::new();
            for choice in &group.0 {
                // special case for homogenous arrays
                if let Some((entry, _has_comma)) = choice.0.first() {
                    let element_type = match entry {
                        GroupEntry::ValueMemberKey(vmk) => rust_type(global, &vmk.entry_type),
                        GroupEntry::TypeGroupname(tgn) => Some(tgn.name.to_string()),
                        _ => Some(format!("UNSUPPORTED_ARRAY_ELEMENT<{:?}>", entry)),
                    }.unwrap();
                    s.push_str(&format!("{}s", element_type));
                    global.generate_array_type(element_type);
                } else {
                    // TODO: how do we handle this? tuples?
                    // or creating a struct definition and referring to it
                    // by name?
                }
                // TODO: handle group choices (enums?)
                break;
            }
            Some(s)
        },
        _ => None,
    }
}

fn rust_type(global: &mut GlobalScope, t: &Type) -> Option<String> {
    for type1 in t.0.iter() {
        // ignoring range control operator here, only interested in Type2
        return rust_type_from_type2(global, &type1.type2);

        // TODO: how to handle type choices? define an enum for every option?
        //       deserializing would be more complicated since you'd
        //       have to test them until one matches.
    }
    panic!("rust_type() is broken for: '{}'", t)
}

fn group_entry_to_type_name(global: &mut GlobalScope, entry: &GroupEntry) -> Option<String> {
    let ret = match entry {
        GroupEntry::ValueMemberKey(vmk) => rust_type(global, &vmk.entry_type),//convert_types(&vmk.entry_type.to_string()).to_owned(),
        GroupEntry::TypeGroupname(tge) => Some(convert_types(&tge.name.to_string()).to_owned()),
        GroupEntry::InlineGroup(_) => panic!("not implemented"),
    };
    println!("group_entry_to_typename({:?}) = {:?}\n", entry, ret);
    ret
}

fn convert_to_wasm_friendly_arg<'a>(type_aliases: &'a BTreeMap<String, String>, arg_type: &'a str) -> &'a str {
    match type_aliases.get(arg_type) {
        Some(alias) => convert_to_wasm_friendly_arg(type_aliases, alias),
        None => match arg_type {
            "Bytes" => "Vec<u8>",
            x => x,
        },
    }
}

// as_map = true generates as map-serialized, and false as array-serialized
fn codegen_group_exposed(global: &mut GlobalScope, type_aliases: &BTreeMap<String, String>, group: &Group, name: &str, as_map: bool) {
    let mut s = codegen::Struct::new(name);
    s
        .vis("pub")
        .field("group", format!("groups::{}", name))
        .derive("Clone");
    let mut ser_impl = codegen::Impl::new(name);
    ser_impl.impl_trait("cbor_event::se::Serialize");
    let mut ser_func = make_serialization_function("serialize");
    let mut group_impl = codegen::Impl::new(name);
    let to_bytes = group_impl.new_fn("to_bytes")
        .ret("Vec<u8>")
        .arg_ref_self()
        .vis("pub");
    if as_map {
        ser_func.line("self.group.serialize_as_map(serializer)");
    } else {
        ser_func.line("self.group.serialize_as_array(serializer)");
    }
    to_bytes.line("let mut buf = Serializer::new_vec();");
    to_bytes.line("self.serialize(&mut buf).unwrap();");
    to_bytes.line("buf.finalize()");
    ser_impl.push_fn(ser_func);
    // TODO: write accessors here? would be common with codegen_group_as_array
    if group.0.len() == 1 {
        let group_choice = group.0.first().unwrap();
        let table_types = table_domain_range(group_choice);
        match table_types {
            Some((domain, range)) => {
                // TODO: how to handle constructors for a table?
            },
            None => {
                let mut new_func = codegen::Function::new("new");
                new_func
                    .ret(name)
                    .vis("pub");
                let mut new_func_block = codegen::Block::new(name);
                let mut output_comma = false;
                let mut args = format!("group: groups::{}::new(", name);
                for (index, (group_entry, _has_comma)) in group_choice.0.iter().enumerate() {
                    let field_name = group_entry_to_field_name(group_entry, index);
                    // Unsupported types so far are fixed values, only have fields
                    // for these.
                    if let Some(type_name) = group_entry_to_type_name(global, group_entry) {
                        if output_comma {
                            args.push_str(", ");
                        } else {
                            output_comma = true;
                        }
                        // TODO: what about genuinely optional types? or maps? we should get that working properly at some point
                        new_func.arg(&field_name, convert_to_wasm_friendly_arg(type_aliases, &type_name));
                        args.push_str(&format!("Some({}.into())", field_name));
                    }
                }
                args.push_str(")");
                new_func_block.line(args);
                new_func.push_block(new_func_block);
                group_impl.push_fn(new_func);
            }
        }
    } else {
        for (i, group_choice) in group.0.iter().enumerate() {
            let variant_name = name.to_owned() + &i.to_string();
            let mut new_func = codegen::Function::new(&format!("new_{}", variant_name));
            new_func
                .ret("Self")
                .vis("pub");
            let mut new_func_block = codegen::Block::new(name);
            let mut output_comma = false;
            let mut args = format!("group: groups::{}::{}(groups::{}::new(", name, variant_name, variant_name);
            for (index, (group_entry, _has_comma)) in group_choice.0.iter().enumerate() {
                let field_name = group_entry_to_field_name(group_entry, index);
                // Unsupported types so far are fixed values, only have fields
                // for these.
                if let Some(type_name) = group_entry_to_type_name(global, group_entry) {
                    if output_comma {
                        args.push_str(", ");
                    } else {
                        output_comma = true;
                    }
                    // TODO: what about genuinely optional types? or maps? we should get that working properly at some point
                    new_func.arg(&field_name, convert_to_wasm_friendly_arg(type_aliases, &type_name));
                    args.push_str(&format!("Some({}.into())", field_name));
                }
            }
            args.push_str("))");
            new_func_block.line(args);
            new_func.push_block(new_func_block);
            group_impl.push_fn(new_func);
        }
    }
    global.scope().raw("#[wasm_bindgen]");
    global.scope().push_struct(s);
    global.scope().push_impl(ser_impl);
    global.scope().raw("#[wasm_bindgen]");
    global.scope().push_impl(group_impl);
}

// Separate function for when we support multiple choices as an enum
fn codegen_group(global: &mut GlobalScope, scope: &mut codegen::Scope, group: &Group, name: &str) {
    if group.0.len() == 1 {
        codegen_group_choice(global, scope, group.0.first().unwrap(), name);
    } else {
        let mut e = codegen::Enum::new(name);
        e
            .vis("pub (super)")
            .derive("Clone");
        let mut e_impl = codegen::Impl::new(name);
        // TODO: serialize map. this is an issue since the implementations might not exist.
        let mut ser_array = make_serialization_function("serialize_as_array");
        ser_array.vis("pub (super)");
        let mut match_block = codegen::Block::new("match self");
        for (i, group_choice) in group.0.iter().enumerate() {
            let variant_name = name.to_owned() + &i.to_string();
            e.push_variant(codegen::Variant::new(&format!("{}({})", variant_name, variant_name)));
            codegen_group_choice(global, scope, group_choice, &variant_name);
            match_block.line(format!("{}::{}(x) => x.serialize_as_array(serializer),", name, variant_name));
        }
        ser_array.push_block(match_block);
        e_impl.push_fn(ser_array);
        scope.push_enum(e);
        scope.push_impl(e_impl);
    }
}

fn table_domain_range(group_choice: &GroupChoice) -> Option<(&Type2, &Type)> {
    if group_choice.0.len() == 1 {
        if let Some((GroupEntry::ValueMemberKey(vmk), _)) = group_choice.0.first() {
            match &vmk.member_key {
                // TODO: Do we need to handle cuts for what we're doing?
                // Does the range control operator matter?
                Some(MemberKey::Type1(type1)) => Some((&type1.0.type2, &vmk.entry_type)),
                _ => None
            }
        } else {
            None
        }
    } else {
        None
    }
}

fn make_serialization_function(name: &str) -> codegen::Function {
    let mut f = codegen::Function::new(name);
    f
        .generic("'se, W: Write")
        .ret("cbor_event::Result<&'se mut Serializer<W>>")
        .arg_ref_self()
        .arg("serializer", "&'se mut Serializer<W>");
    f
}

fn codegen_group_choice(global: &mut GlobalScope, scope: &mut codegen:: Scope, group_choice: &GroupChoice, name: &str) {
    // handles ValueMemberKey only
    // TODO: TypeGroupname / InlinedGroup are not supported yet
    // TODO: handle non-integer keys (all keys in shelley.cddl are uint)

    let s = scope.new_struct(name);
    s
        .vis("pub (super)")
        .derive("Clone");
    let mut s_impl = codegen::Impl::new(name);
    // We could re-use this for arrays I guess and add a tag?

    // Here we test if this is a struct vs a table.
    // struct: { x: int, y: int }, etc
    // table: { * int => tstr }, etc
    // this assumes that all maps representing tables are homogenous
    // and contain no other fields. I am not sure if this is a guarantee in
    // cbor but I would hope that the cddl specs we are using follow this.

    // Is there a more concise/readable way of expressing this in rust?
    let table_types = table_domain_range(group_choice);
    match table_types {
        Some((domain, range)) => {
            s.field("table", format!("std::collections::BTreeMap<{}, {}>", rust_type_from_type2(global, domain).unwrap(), rust_type(global, range).unwrap()));
            let mut ser_map = make_serialization_function("serialize_as_map");
            ser_map
                .vis("pub (super)")
                .line("panic!(\"TODO: implement\");");
            s_impl.push_fn(ser_map);
        },
        None => {
            let mut ser_array = make_serialization_function("serialize_as_array");
            let mut ser_map = make_serialization_function("serialize_as_map");
            ser_array
                .vis("pub (super)")
                .line(format!("serializer.write_array(cbor_event::Len::Len({}u64))?;", group_choice.0.len()));
            ser_map
                .vis("pub (super)")
                .line(format!("serializer.write_array(cbor_event::Len::Len({}u64))?;", group_choice.0.len()));
            // If we have a group with entries that have no names, that's fine for arrays
            // but not for maps, so if we encounter one assume we should not generate
            // map-related functions.
            // In the future we could change this tool to only emit the array or map
            // functions when they are strictly necessary (wrapped in array or map elsewhere)
            // This would also reduce error checking here since we wouldn't hit certain cases
            let mut contains_entries_without_names = false;
            let mut new_func = codegen::Function::new("new");
            new_func
                .ret("Self")
                .vis("pub (super)");
            let mut new_func_block = codegen::Block::new(name);
            for (index, (group_entry, _has_comma)) in group_choice.0.iter().enumerate() {
                let field_name = group_entry_to_field_name(group_entry, index);
                // Unsupported types so far are fixed values, only have fields
                // for these.
                if let Some(type_name) = group_entry_to_type_name(global, group_entry) {
                    s.field(&field_name, format!("Option<{}>", type_name));
                    // TODO: what about genuinely optional types? or maps? we should get that working properly at some point
                    new_func.arg(&field_name, format!("Option<{}>", type_name));
                    new_func_block.line(format!("{}: {},", field_name, field_name));
                    // TODO: support conditional members (100% necessary for heterogenous maps (not tables))
                    // TODO: proper support since this assumes all members implement the trait
                    //       maybe we could put a special case for primitives or Maps/Vecs?
                    // TODO: remove clone()? Without it String gets moved out.
                    ser_array.line(format!("self.{}.clone().unwrap().serialize(serializer)?;", field_name));
                    match group_entry {
                        GroupEntry::ValueMemberKey(vmk) => {
                            match vmk.member_key.as_ref() {
                                Some(member_key) => match member_key {
                                    MemberKey::Value(value) => match value {
                                        cddl::token::Value::UINT(x) => {
                                            ser_map.line(format!("serializer.write_unsigned_integer({})?;", x));
                                        },
                                        _ => panic!("unsupported map identifier(1): {:?}", value),
                                    },
                                    MemberKey::Bareword(ident) => {
                                        ser_map.line(format!("serializer.write_text(\"{}\")?;", ident.to_string()));
                                    },
                                    x => panic!("unsupported map identifier(2): {:?}", x),
                                },
                                None => {
                                    contains_entries_without_names = true;
                                },
                            }
                        },
                        // TODO: why are we hitting this?
                        // GroupEntry::TypeGroupname(tgn) => match tgn.name.to_string().as_ref() {
                        //     "uint" => format!("serializer.write_unsigned_integer({})?;", x),
                        //     x => panic!("TODO: serialize '{}'", x),
                        // },
                        _ => {
                            //panic!("unsupported map identifier(3): {:?}", x),
                            // TODO: only generate map vs array stuff when needed to avoid this hack
                            contains_entries_without_names = true;
                        },
                    };
                    ser_map.line(format!("self.{}.clone().unwrap().serialize(serializer)?;", field_name));
                } else {
                    // TODO: do we need to support type choices here?!
                    match group_entry {
                        GroupEntry::ValueMemberKey(vmk) => match vmk.entry_type.0.first() {
                            Some(x) => match &x.type2 {
                                Type2::UintValue(x) => {
                                    ser_array.line(format!("serializer.write_unsigned_integer({})?;", x));
                                },
                                x => panic!("unsupported fixed type: {}", x),
                            },
                            None => unreachable!(),
                        },
                        _ => panic!("unsupported fixed type: {:?}", group_entry),
                    }
                }
            }
            ser_array.line("Ok(serializer)");
            ser_map.line("Ok(serializer)");
            //ser_array.line("serializer.write_special(cbor_event::Special::Break)");
            //ser_map.line("serializer.write_special(cbor_event::Special::Break)");
            new_func.push_block(new_func_block);
            s_impl.push_fn(new_func);
            s_impl.push_fn(ser_array);
            if !contains_entries_without_names {
                s_impl.push_fn(ser_map);
            }
        }
    }
    scope.push_impl(s_impl);
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut type_aliases = BTreeMap::<String, String>::new();
    let cddl_in = std::fs::read_to_string("supported.cddl").unwrap();
    let cddl = cddl::parser::cddl_from_str(&cddl_in)?;
    //println!("CDDL file: {}", cddl);
    let mut global = GlobalScope::new();
    // Can't generate groups of imports with codegen::Import so we just output this as raw text
    // since we don't need it to be dynamic so it's fine. codegen::Impl::new("a", "{z::b, z::c}")
    // does not work.
    global.scope().raw("// This library was code-generated using an experimental CDDL to rust tool:\n// https://github.com/Emurgo/cardano-serialization-lib/tree/master/cddl_test");
    global.scope().raw("use cbor_event::{self, de::{Deserialize, Deserializer}, se::{Serialize, Serializer}};");
    global.scope().import("std::io", "Write");
    global.scope().import("wasm_bindgen::prelude", "*");
    // TODO: maybe export this as a prelude.rs file instead of creating it in code since it's static
    // We need wrapper types for arrays/bytes as we can't specialize Vec<T> to cbor_event's Serialize
    // as they come from different external crates.
    // global.scope().new_struct("Array<T>(Vec<T>)");
    // global.scope()
    //     .new_impl("Array<T>")
    //     .generic("T")
    //     .impl_trait("std::ops::Deref")
    //     .associate_type("Target", "Vec<T>")
    //     .new_fn("deref")
    //     .arg_ref_self()
    //     .ret("&Vec<T>")
    //     .line("&self.0");
    global.scope().raw("#[wasm_bindgen]");
    global.scope()
        .new_struct("Bytes(Vec<u8>)")
        .derive("Clone")
        .vis("pub");
    global.scope().raw("#[wasm_bindgen]");
    global.scope()
        .new_impl("Bytes")
        .new_fn("new")
        .vis("pub")
        .arg("data", "&[u8]")
        .ret("Self")
        .line("Self(data.into())");
    global.scope()
        .new_impl("Bytes")
        .impl_trait("From<Vec<u8>>")
        .new_fn("from")
        .arg("data", "Vec<u8>")
        .ret("Self")
        .line("Self(data)");
    global.scope()
        .new_impl("Bytes")
        .impl_trait("Serialize")
        .new_fn("serialize")
        .arg_ref_self()
        .arg("serializer", "&'a mut Serializer<W>")
        .generic("'a, W: Write + Sized")
        .ret("cbor_event::Result<&'a mut Serializer<W>>")
        .line("serializer.write_bytes(&self.0[..])");
    let mut group_module = codegen::Module::new("groups");
    let group_scope = group_module.scope();
    group_scope.import("super", "*");
    for rule in cddl.rules {
        match rule {
            Rule::Type(tr) => {
                // (1) does not handle optional generic parameters
                // (2) does not handle ranges - I think they're the character position in the CDDL
                // (3) is_type_choice_alternate ignored since shelley cddl doesn't need it
                //     It's used, but used for no reason as it is the initial definition
                //     (which is also valid cddl), but it would be fine as = instead of /=
                // TODO: choices (as enums I guess?)
                for choice in &tr.value.0 {
                    // ignores control operators - only used in shelley spec to limit string length for application metadata
                    match &choice.type2 {
                        Type2::Typename((identifier, _generic_arg)) => {
                            let alias = tr.name.to_string();
                            let base_type = convert_types(&identifier.to_string()).to_owned();
                            global.scope().raw(format!("type {} = {};", alias, base_type).as_ref());
                            type_aliases.insert(alias, base_type);
                        },
                        Type2::Map(group) => {
                            let group_name = tr.name.to_string();
                            codegen_group(&mut global, group_scope, group, group_name.as_ref());
                            codegen_group_exposed(&mut global, &type_aliases, group, group_name.as_ref(), true);
                        },
                        Type2::Array(group) => {
                            let group_name = tr.name.to_string();
                            codegen_group(&mut global, group_scope, group, group_name.as_ref());
                            codegen_group_exposed(&mut global, &type_aliases, group, group_name.as_ref(), false);
                        },
                        x => {
                            println!("\nignored typename {} -> {:?}\n", tr.name, x);
                            // ignored
                        }
                    }
                    //println!("{} type2 = {:?}\n", tr.name, choice.type2);
                    //s.field("foo", "usize");
                    // remove and implement type choices
                    break;
                }
            },
            Rule::Group(group_rule) => {
                // Freely defined group - no need to generate anything outside of group module
                match &group_rule.entry {
                    GroupEntry::InlineGroup((_occsur, inline_group)) => {
                        codegen_group(&mut global, group_scope, inline_group, &group_rule.name.to_string());
                    },
                    x => panic!("Group rule with non-inline group? {:?}", x),
                }
            },
        }
    }
    global.scope().push_module(group_module);
    match std::fs::remove_dir_all("export/src") {
        Ok(()) => (),
        Err(_) => (),
    };
    std::fs::create_dir_all("export/src").unwrap();
    std::fs::write("export/src/lib.rs", global.scope().to_string()).unwrap();
    std::fs::copy("static/Cargo.toml", "export/Cargo.toml").unwrap();
    std::fs::copy("static/prelude.rs", "export/src/prelude.rs").unwrap();

    Ok(())
}
