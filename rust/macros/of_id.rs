// SPDX-License-Identifier: GPL-2.0

use proc_macro::{TokenStream, TokenTree};

pub(crate) fn derive_get_id_info(ts: TokenStream) -> TokenStream {
    let tokens: Vec<_> = ts.into_iter().collect();
    let mut type_name = String::from("Struct");

    tokens
        .iter()
        .find_map(|token| match token {
            TokenTree::Ident(ident) => match ident.to_string().as_str() {
                "struct" => Some(1),
                _ => None,
            },
            _ => None,
        })
        .expect("#[GetIdInfo] should be applied to a struct");

    let mut content = tokens.into_iter();
    while let Some(token) = content.next() {
        match token {
            TokenTree::Ident(ident) if ident.to_string() == "struct" => {
                match content.next() {
                    Some(TokenTree::Ident(ident)) => type_name = ident.to_string(),
                    // This will never happend.
                    _ => (),
                };
            }
            _ => (),
        }
    }

    println!("value {:?}", type_name);

    format!(
        "
            impl<T: Driver> {name}<T> {{
                fn get_id_info(dev: &Device) -> Option<&'static T::IdInfo> {{
                    let table = T::OF_DEVICE_ID_TABLE?;

                    // SAFETY: `table` has static lifetime, so it is valid for read. `dev` is guaranteed to be
                    // valid while it's alive, so is the raw device returned by it.
                    let id = unsafe {{ bindings::of_match_device(table.as_ref(), dev.raw_device()) }};
                    if id.is_null() {{
                        return None;
                    }}

                    // SAFETY: `id` is a pointer within the static table, so it's always valid.
                    let offset = unsafe {{ (*id).data }};
                    if offset.is_null() {{
                        return None;
                    }}

                    // SAFETY: The offset comes from a previous call to `offset_from` in `IdArray::new`, which
                    // guarantees that the resulting pointer is within the table.
                    let ptr = unsafe {{
                        id.cast::<u8>()
                            .offset(offset as _)
                            .cast::<Option<T::IdInfo>>()
                    }};

                    // SAFETY: The id table has a static lifetime, so `ptr` is guaranteed to be valid for read.
                    unsafe {{ (&*ptr).as_ref() }}
                }}
            }}
        ",
        name = type_name,
    ).parse().expect("Error parsing formatted string into token stream.")
}
