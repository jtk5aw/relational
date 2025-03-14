use proc_macro::TokenStream;
use quote::{quote, format_ident};
use syn::{parse_macro_input, DeriveInput, Data, Fields, Ident, Type, TypePath, PathArguments};

#[proc_macro_derive(StructLayout)]
pub fn derive_struct_layout(input: TokenStream) -> TokenStream {
    // Parse the input tokens into a syntax tree
    let input = parse_macro_input!(input as DeriveInput);
    
    // Get the name of the struct
    let struct_name = &input.ident;
    
    // Generate the impl block
    let expanded = generate_impl(struct_name, &input.data);
    
    // Return the generated code
    TokenStream::from(expanded)
}

// Function to check if a type is a primitive that we want to generate layout for
// Now also accepts enum types as "primitives"
fn is_primitive_or_enum_type(ty: &Type) -> bool {
    if let Type::Path(TypePath { path, .. }) = ty {
        if let Some(segment) = path.segments.first() {
            let type_name = segment.ident.to_string();
            
            // List of primitive types we want to support
            let primitives = [
                "u8", "u16", "u32", "u64", "u128", "usize",
                "i8", "i16", "i32", "i64", "i128", "isize",
                "f32", "f64", "bool", "char"
            ];
            
            // Check if it's a primitive
            if primitives.contains(&type_name.as_str()) {
                return true;
            }
            
            // Check if it's an enum (no type parameters)
            // This is a heuristic - if it's a user-defined type with no type parameters,
            // we'll consider it potentially an enum and include it
            if !primitives.contains(&type_name.as_str()) && matches!(segment.arguments, PathArguments::None) {
                return true;
            }
        }
    }
    false
}

fn generate_impl(struct_name: &Ident, data: &Data) -> proc_macro2::TokenStream {
    match data {
        Data::Struct(data_struct) => {
            match &data_struct.fields {
                Fields::Named(fields) => {
                    // Check if there are any complex types followed by primitive/enum types
                    let mut found_complex_type = false;
                    let mut invalid_field_after_complex = None;

                    for field in fields.named.iter() {
                        let is_primitive_or_enum = is_primitive_or_enum_type(&field.ty);
                        
                        if !is_primitive_or_enum {
                            found_complex_type = true;
                        } else if found_complex_type {
                            // Found a primitive/enum after a complex type
                            invalid_field_after_complex = field.ident.as_ref().map(|ident| ident.to_string());
                            break;
                        }
                    }

                    // If we found a primitive/enum after a complex type, return an error
                    if let Some(field_name) = invalid_field_after_complex {
                        let error = format!("StructLayout does not support primitive/enum fields after complex types. Field '{}' is invalid.", field_name);
                        return quote! {
                            compile_error!(#error);
                        };
                    }
                    
                    // Check if all fields are primitives or enums
                    let all_primitives_or_enums = fields.named.iter()
                        .all(|field| is_primitive_or_enum_type(&field.ty));
                    
                    // Generate field size constants for primitive/enum types only
                    let field_size_constants = fields.named.iter().filter_map(|field| {
                        let field_ident = field.ident.as_ref()?;
                        let field_ty = &field.ty;
                        
                        // Skip non-primitive/non-enum types
                        if !is_primitive_or_enum_type(field_ty) {
                            return None;
                        }
                        
                        let const_name = format_ident!("{}_SIZE", field_ident.to_string().to_uppercase());
                        
                        Some(quote! {
                            /// The size in bytes of this field
                            pub const #const_name: usize = std::mem::size_of::<#field_ty>();
                        })
                    });
                    
                    // Generate field offset constants for primitive/enum types only
                    let field_offset_constants = fields.named.iter().filter_map(|field| {
                        let field_ident = field.ident.as_ref()?;
                        let field_ty = &field.ty;
                        
                        // Skip non-primitive/non-enum types
                        if !is_primitive_or_enum_type(field_ty) {
                            return None;
                        }
                        
                        let const_name = format_ident!("{}_OFFSET", field_ident.to_string().to_uppercase());
                        
                        Some(quote! {
                            /// The byte offset of this field within the struct
                            pub const #const_name: usize = memoffset::offset_of!(#struct_name, #field_ident);
                        })
                    });

                    // Generate field span methods for primitive/enum types only
                    let field_span_methods = fields.named.iter().filter_map(|field| {
                        let field_ident = field.ident.as_ref()?;
                        let field_ty = &field.ty;
                        
                        // Skip non-primitive/non-enum types
                        if !is_primitive_or_enum_type(field_ty) {
                            return None;
                        }
                        
                        let method_name = format_ident!("{}_span", field_ident);
                        
                        Some(quote! {
                            /// Returns the byte range that this field spans within the struct
                            pub fn #method_name() -> std::ops::Range<usize> {
                                memoffset::span_of!(#struct_name, #field_ident)
                            }
                        })
                    });
                    
                    // Generate total size constant and field count only if all fields are primitives/enums
                    let struct_constants = if all_primitives_or_enums {
                        let field_count = fields.named.iter()
                            .filter(|field| is_primitive_or_enum_type(&field.ty))
                            .count();
                            
                        quote! {
                            /// The total size of the struct in bytes
                            pub const SIZE: usize = std::mem::size_of::<#struct_name>();
                            
                            /// The number of primitive/enum fields in the struct
                            pub const FIELD_COUNT: usize = #field_count;
                        }
                    } else {
                        // Only count primitive/enum fields
                        let field_count = fields.named.iter()
                            .filter(|field| is_primitive_or_enum_type(&field.ty))
                            .count();
                            
                        quote! {
                            /// The number of primitive/enum fields in the struct
                            pub const FIELD_COUNT: usize = #field_count;
                        }
                    };
                    
                    // Full implementation
                    quote! {
                        impl #struct_name {
                            // Struct constants (SIZE only if all fields are primitives/enums)
                            #struct_constants
                            
                            // Field size constants (primitives/enums only)
                            #(#field_size_constants)*
                            
                            // Field offset constants (primitives/enums only)
                            #(#field_offset_constants)*
                            
                            // Field span methods (primitives/enums only)
                            #(#field_span_methods)*
                        }
                    }
                },
                _ => {
                    // Only named fields are supported
                    let error = format!("StructLayout only supports structs with named fields");
                    quote! {
                        compile_error!(#error);
                    }
                }
            }
        },
        _ => {
            // Only structs are supported
            let error = format!("StructLayout only supports structs");
            quote! {
                compile_error!(#error);
            }
        }
    }
}
