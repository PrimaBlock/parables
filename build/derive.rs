//! Generate code for contracts using ethabi.
// Copyright 2016-2018 Parity Technologies (UK) Limited
// Copyright 2018 PrimaBlock OÜ
//
// Copied from:
// https://github.com/paritytech/ethabi/blob/33aa6e2a94dc64406bd884c1d7c60c3ddb239af8/derive/src/lib.rs

use ethabi::{self, Constructor, Contract, Event, Function, Param, ParamType, Result};
use heck::{CamelCase, SnakeCase};
use quote;
use serde_json;
use std::collections::HashMap;
use std::fmt;
use std::path::Path;
use syn;

const INTERNAL_ERR: &'static str = "`parables_testing` internal error";

#[derive(Deserialize)]
pub struct ContractFields {
    abi: String,
    bin: String,
    #[serde(rename = "srcmap")]
    source_map: String,
    #[serde(rename = "srcmap-runtime")]
    source_map_runtime: String,
    #[serde(rename = "bin-runtime")]
    bin_runtime: String,
}

#[derive(Deserialize)]
pub struct Output {
    contracts: HashMap<String, ContractFields>,
    #[serde(rename = "sourceList")]
    #[allow(unused)]
    source_list: Vec<String>,
    #[allow(unused)]
    version: String,
}

/// Implement a module for the given output.
pub fn impl_module(path: &Path, output: Output) -> Result<quote::Tokens> {
    let mut result = Vec::new();
    let mut source_maps = Vec::new();

    for (name, contract) in output.contracts {
        let name = parse_name(&name)?;

        let bin_function = bin_function(&contract)?;
        let source_maps_function = source_maps_function(&name, &contract)?;

        let contract = impl_contract_abi(&contract.abi)?;

        let module_name = syn::Ident::from(name.module_name);

        result.push(quote! {
            pub mod #module_name {
                #bin_function

                #source_maps_function

                #contract
            }
        });

        source_maps.push(module_name);
    }

    result.push(source_maps_global_function(
        path,
        output.source_list,
        source_maps,
    ));

    return Ok(quote!{ #(#result)* });

    #[derive(Debug)]
    pub struct Name<'a> {
        path: &'a str,
        module_name: String,
        type_name: &'a str,
    }

    impl<'a> fmt::Display for Name<'a> {
        fn fmt(&self, fmt: &mut fmt::Formatter) -> fmt::Result {
            write!(fmt, "{}:{}", self.path, self.type_name)
        }
    }

    fn parse_name<'a>(name: &'a str) -> Result<Name<'a>> {
        let mut parts = name.split(":");

        let path = parts.next().ok_or_else(|| format!("bad name: {}", name))?;
        let type_name = parts.next().ok_or_else(|| format!("bad name: {}", name))?;

        let module_name = type_name.to_snake_case();

        Ok(Name {
            path,
            module_name,
            type_name,
        })
    }

    fn bin_function(contract: &ContractFields) -> Result<quote::Tokens> {
        let bin = &contract.bin;

        Ok(quote! {
            pub fn bin(linker: &::parables_testing::linker::Linker)
                -> Result<Vec<u8>, ::parables_testing::error::Error>
            {
                linker.link(#bin)
            }
        })
    }

    fn source_maps_global_function(
        path: &Path,
        source_list: Vec<String>,
        source_maps: Vec<syn::Ident>,
    ) -> quote::Tokens {
        let source_list = source_list
            .into_iter()
            .map(|p| path.join(p).display().to_string())
            .collect::<Vec<_>>();

        quote! {
            pub fn source_maps(linker: &mut ::parables_testing::linker::Linker)
                -> ::std::result::Result<(), ::parables_testing::error::Error>
            {
                linker.register_source_list(vec![#(::std::path::Path::new(#source_list).to_owned(),)*]);
                #(#source_maps::source_maps(linker)?;)*
                Ok(())
            }
        }
    }

    fn source_maps_function(name: &Name, contract: &ContractFields) -> Result<quote::Tokens> {
        let name = name.type_name;
        let bin = &contract.bin;
        let source_map = &contract.source_map;
        let source_map_runtime = &contract.source_map_runtime;
        let bin_runtime = &contract.bin_runtime;

        Ok(quote! {
            pub fn source_maps(linker: &mut ::parables_testing::linker::Linker)
                -> Result<(), ::parables_testing::error::Error>
            {
                /*let source_map = ::parables_testing::source_map::SourceMap::parse(#source_map)?;
                let offsets = linker.decode_offsets(#bin)?;

                linker.register_source(
                    #name.to_string(),
                    source_map, offsets);*/

                let runtime_source_map = ::parables_testing::source_map::SourceMap::parse(#source_map_runtime)?;
                let runtime_offsets = linker.decode_offsets(#bin_runtime)?;

                linker.register_runtime_source(
                    #name.to_string(),
                    runtime_source_map, runtime_offsets);

                Ok(())
            }
        })
    }
}

/// Implement the contract ABI.
fn impl_contract_abi(input: &str) -> Result<quote::Tokens> {
    let contract: Contract = serde_json::from_str(input)?;

    let functions: Vec<_> = contract.functions().map(impl_contract_function).collect();
    let events_impl: Vec<_> = contract.events().map(impl_contract_event).collect();
    let constructor_impl = impl_contract_constructor(contract.constructor.as_ref());
    let constructor_input_wrapper_struct =
        declare_contract_constructor_input_wrapper(contract.constructor.as_ref());
    let logs_structs: Vec<_> = contract.events().map(declare_logs).collect();
    let events_structs: Vec<_> = contract.events().map(declare_events).collect();
    let func_structs: Vec<_> = contract.functions().map(declare_functions).collect();
    let output_functions: Vec<_> = contract.functions().map(declare_output_functions).collect();
    let func_input_wrappers_structs: Vec<_> = contract
        .functions()
        .map(declare_functions_input_wrappers)
        .collect();

    let events_and_logs_quote = if events_structs.is_empty() {
        quote!{}
    } else {
        quote! {
            pub mod events {
                use parables_testing::ethabi;
                use parables_testing::ethabi::ParseLog;
                use parables_testing::ethabi::LogFilter;

                #(#events_structs)*

                #(#events_impl)*
            }

            pub mod logs {
                use parables_testing::ethabi;

                #(#logs_structs)*
            }
        }
    };

    let functions_quote = if func_structs.is_empty() {
        quote!{}
    } else {
        quote! {
            pub mod functions {
                use parables_testing::ethabi;

                #(#func_structs)*

                #(#functions)*

                #(#func_input_wrappers_structs)*
            }
        }
    };

    let outputs_quote = if output_functions.is_empty() {
        quote!{}
    } else {
        quote! {
            /// Contract functions (for decoding output)
            pub mod outputs {
                use parables_testing::ethabi;

                #(#output_functions)*
            }
        }
    };

    let result = quote! {
        // may not be used
        use parables_testing::ethabi;

        #constructor_impl

        #constructor_input_wrapper_struct

        #events_and_logs_quote

        #outputs_quote

        #functions_quote
    };

    Ok(result)
}

fn to_syntax_string(param_type: &ethabi::ParamType) -> quote::Tokens {
    match *param_type {
        ParamType::Address => quote! { ethabi::ParamType::Address },
        ParamType::Bytes => quote! { ethabi::ParamType::Bytes },
        ParamType::Int(x) => quote! { ethabi::ParamType::Int(#x) },
        ParamType::Uint(x) => quote! { ethabi::ParamType::Uint(#x) },
        ParamType::Bool => quote! { ethabi::ParamType::Bool },
        ParamType::String => quote! { ethabi::ParamType::String },
        ParamType::Array(ref param_type) => {
            let param_type_quote = to_syntax_string(param_type);
            quote! { ethabi::ParamType::Array(Box::new(#param_type_quote)) }
        }
        ParamType::FixedBytes(x) => quote! { ethabi::ParamType::FixedBytes(#x) },
        ParamType::FixedArray(ref param_type, ref x) => {
            let param_type_quote = to_syntax_string(param_type);
            quote! { ethabi::ParamType::FixedArray(Box::new(#param_type_quote), #x) }
        }
    }
}

fn to_ethabi_param_vec<'a, P: 'a>(params: P) -> quote::Tokens
where
    P: IntoIterator<Item = &'a Param>,
{
    let p = params
        .into_iter()
        .map(|x| {
            let name = &x.name;
            let kind = to_syntax_string(&x.kind);
            quote! {
                ethabi::Param {
                    name: #name.to_owned(),
                    kind: #kind
                }
            }
        })
        .collect::<Vec<_>>();

    quote! { vec![ #(#p),* ] }
}

fn rust_type(input: &ParamType) -> quote::Tokens {
    match *input {
        ParamType::Address => quote! { ethabi::Address },
        ParamType::Bytes => quote! { ethabi::Bytes },
        ParamType::FixedBytes(32) => quote! { ethabi::Hash },
        ParamType::FixedBytes(size) => quote! { [u8; #size] },
        ParamType::Int(_) => quote! { ethabi::Int },
        ParamType::Uint(_) => quote! { ethabi::Uint },
        ParamType::Bool => quote! { bool },
        ParamType::String => quote! { String },
        ParamType::Array(ref kind) => {
            let t = rust_type(&*kind);
            quote! { Vec<#t> }
        }
        ParamType::FixedArray(ref kind, size) => {
            let t = rust_type(&*kind);
            quote! { [#t, #size] }
        }
    }
}

fn template_param_type(input: &ParamType, index: usize) -> quote::Tokens {
    let t_ident = syn::Ident::from(format!("T{}", index));
    let u_ident = syn::Ident::from(format!("U{}", index));
    match *input {
        ParamType::Address => quote! { #t_ident: Into<ethabi::Address> },
        ParamType::Bytes => quote! { #t_ident: Into<ethabi::Bytes> },
        ParamType::FixedBytes(32) => quote! { #t_ident: Into<ethabi::Hash> },
        ParamType::FixedBytes(size) => quote! { #t_ident: Into<[u8; #size]> },
        ParamType::Int(_) => quote! { #t_ident: Into<ethabi::Int> },
        ParamType::Uint(_) => quote! { #t_ident: Into<ethabi::Uint> },
        ParamType::Bool => quote! { #t_ident: Into<bool> },
        ParamType::String => quote! { #t_ident: Into<String> },
        ParamType::Array(ref kind) => {
            let t = rust_type(&*kind);
            quote! {
                #t_ident: IntoIterator<Item = #u_ident>, #u_ident: Into<#t>
            }
        }
        ParamType::FixedArray(ref kind, size) => {
            let t = rust_type(&*kind);
            quote! {
                #t_ident: Into<[#u_ident; #size]>, #u_ident: Into<#t>
            }
        }
    }
}

fn from_template_param(input: &ParamType, name: &syn::Ident) -> quote::Tokens {
    match *input {
        ParamType::Array(_) => quote! { #name.into_iter().map(Into::into).collect::<Vec<_>>() },
        ParamType::FixedArray(_, _) => quote! { (Box::new(#name.into()) as Box<[_]>).into_vec().into_iter().map(Into::into).collect::<Vec<_>>() },
        _ => quote! {#name.into() },
    }
}

fn to_token(name: &quote::Tokens, kind: &ParamType) -> quote::Tokens {
    match *kind {
        ParamType::Address => quote! { ethabi::Token::Address(#name) },
        ParamType::Bytes => quote! { ethabi::Token::Bytes(#name) },
        ParamType::FixedBytes(_) => quote! { ethabi::Token::FixedBytes(#name.to_vec()) },
        ParamType::Int(_) => quote! { ethabi::Token::Int(#name) },
        ParamType::Uint(_) => quote! { ethabi::Token::Uint(#name) },
        ParamType::Bool => quote! { ethabi::Token::Bool(#name) },
        ParamType::String => quote! { ethabi::Token::String(#name) },
        ParamType::Array(ref kind) => {
            let inner_name = quote! { inner };
            let inner_loop = to_token(&inner_name, kind);
            quote! {
                // note the double {{
                {
                    let v = #name.into_iter().map(|#inner_name| #inner_loop).collect();
                    ethabi::Token::Array(v)
                }
            }
        }
        ParamType::FixedArray(ref kind, _) => {
            let inner_name = quote! { inner };
            let inner_loop = to_token(&inner_name, kind);
            quote! {
                // note the double {{
                {
                    let v = #name.into_iter().map(|#inner_name| #inner_loop).collect();
                    ethabi::Token::FixedArray(v)
                }
            }
        }
    }
}

fn from_token(kind: &ParamType, token: &quote::Tokens) -> quote::Tokens {
    match *kind {
        ParamType::Address => quote! { #token.to_address().expect(#INTERNAL_ERR) },
        ParamType::Bytes => quote! { #token.to_bytes().expect(#INTERNAL_ERR) },
        ParamType::FixedBytes(32) => quote! {
            {
                let mut result = [0u8; 32];
                let v = #token.to_fixed_bytes().expect(#INTERNAL_ERR);
                result.copy_from_slice(&v);
                ethabi::Hash::from(result)
            }
        },
        ParamType::FixedBytes(size) => {
            let size: syn::Index = size.into();
            quote! {
                {
                    let mut result = [0u8; #size];
                    let v = #token.to_fixed_bytes().expect(#INTERNAL_ERR);
                    result.copy_from_slice(&v);
                    result
                }
            }
        }
        ParamType::Int(_) => quote! { #token.to_int().expect(#INTERNAL_ERR) },
        ParamType::Uint(_) => quote! { #token.to_uint().expect(#INTERNAL_ERR) },
        ParamType::Bool => quote! { #token.to_bool().expect(#INTERNAL_ERR) },
        ParamType::String => quote! { #token.to_string().expect(#INTERNAL_ERR) },
        ParamType::Array(ref kind) => {
            let inner = quote! { inner };
            let inner_loop = from_token(kind, &inner);
            quote! {
                #token.to_array().expect(#INTERNAL_ERR).into_iter()
                    .map(|#inner| #inner_loop)
                    .collect()
            }
        }
        ParamType::FixedArray(ref kind, size) => {
            let inner = quote! { inner };
            let inner_loop = from_token(kind, &inner);
            let to_array = vec![quote! { iter.next() }; size];
            quote! {
                {
                    let iter = #token.to_array().expect(#INTERNAL_ERR).into_iter()
                        .map(|#inner| #inner_loop);
                    [#(#to_array),*]
                }
            }
        }
    }
}

fn input_names(inputs: &Vec<Param>) -> Vec<syn::Ident> {
    inputs
        .iter()
        .enumerate()
        .map(|(index, param)| {
            if param.name.is_empty() {
                syn::Ident::from(format!("param{}", index))
            } else {
                rust_variable(&param.name).into()
            }
        })
        .collect()
}

fn get_template_names(kinds: &Vec<quote::Tokens>) -> Vec<syn::Ident> {
    kinds
        .iter()
        .enumerate()
        .map(|(index, _)| syn::Ident::from(format!("T{}", index)))
        .collect()
}

fn get_output_kinds(outputs: &Vec<Param>) -> quote::Tokens {
    match outputs.len() {
        0 => quote! {()},
        1 => {
            let t = rust_type(&outputs[0].kind);
            quote! { #t }
        }
        _ => {
            let outs: Vec<_> = outputs.iter().map(|param| rust_type(&param.kind)).collect();
            quote! { (#(#outs),*) }
        }
    }
}

fn impl_contract_function(function: &Function) -> quote::Tokens {
    let name = syn::Ident::from(function.name.to_snake_case());
    let function_input_wrapper_name =
        syn::Ident::from(format!("{}WithInput", function.name.to_camel_case()));

    // [param0, hello_world, param2]
    let ref input_names: Vec<_> = input_names(&function.inputs);

    // [T0: Into<Uint>, T1: Into<Bytes>, T2: IntoIterator<Item = U2>, U2 = Into<Uint>]
    let ref template_params: Vec<_> = function
        .inputs
        .iter()
        .enumerate()
        .map(|(index, param)| template_param_type(&param.kind, index))
        .collect();

    // [Uint, Bytes, Vec<Uint>]
    let kinds: Vec<_> = function
        .inputs
        .iter()
        .map(|param| rust_type(&param.kind))
        .collect();

    // [T0, T1, T2]
    let template_names: Vec<_> = get_template_names(&kinds);

    // [param0: T0, hello_world: T1, param2: T2]
    let ref params: Vec<_> = input_names
        .iter()
        .zip(template_names.iter())
        .map(|(param_name, template_name)| quote! { #param_name: #template_name })
        .collect();

    // [Token::Uint(param0.into()), Token::Bytes(hello_world.into()), Token::Array(param2.into_iter().map(Into::into).collect())]
    let usage: Vec<_> = input_names
        .iter()
        .zip(function.inputs.iter())
        .map(|(param_name, param)| {
            to_token(&from_template_param(&param.kind, &param_name), &param.kind)
        })
        .collect();

    quote! {
        /// Sets the input (arguments) for this contract function
        pub fn #name<#(#template_params),*>(#(#params),*) -> #function_input_wrapper_name {
            let v: Vec<ethabi::Token> = vec![#(#usage),*];
            #function_input_wrapper_name::new(v)
        }
    }
}

fn impl_contract_event(event: &Event) -> quote::Tokens {
    let name = syn::Ident::from(event.name.to_snake_case());
    let event_name = syn::Ident::from(event.name.to_camel_case());
    quote! {
        pub fn #name() -> super::events::#event_name {
            super::events::#event_name::default()
        }
    }
}

fn impl_contract_constructor(constructor: Option<&Constructor>) -> quote::Tokens {
    // [param0, hello_world, param2]
    let input_names: Vec<_> = constructor
        .map(|c| input_names(&c.inputs))
        .unwrap_or_else(Vec::new);

    // [Uint, Bytes, Vec<Uint>]
    let kinds: Vec<_> = constructor
        .map(|c| {
            c.inputs
                .iter()
                .map(|param| rust_type(&param.kind))
                .collect()
        })
        .unwrap_or_else(Vec::new);

    // [T0, T1, T2]
    let template_names: Vec<_> = get_template_names(&kinds);

    // [T0: Into<Uint>, T1: Into<Bytes>, T2: IntoIterator<Item = U2>, U2 = Into<Uint>]
    let template_params: Vec<_> = constructor
        .map(|c| {
            c.inputs
                .iter()
                .enumerate()
                .map(|(index, param)| template_param_type(&param.kind, index))
                .collect()
        })
        .unwrap_or_else(Vec::new);

    // [param0: T0, hello_world: T1, param2: T2]
    let params: Vec<_> = input_names
        .iter()
        .zip(template_names.iter())
        .map(|(param_name, template_name)| quote! { #param_name: #template_name })
        .collect();

    // [Token::Uint(param0.into()), Token::Bytes(hello_world.into()), Token::Array(param2.into())]
    let usage: Vec<_> = input_names
        .iter()
        .zip(constructor.iter().flat_map(|c| c.inputs.iter()))
        .map(|(param_name, param)| {
            to_token(&from_template_param(&param.kind, &param_name), &param.kind)
        })
        .collect();

    quote! {
        pub fn constructor<#(#template_params),*>(code: ethabi::Bytes, #(#params),* ) -> ConstructorWithInput {
            let v: Vec<ethabi::Token> = vec![#(#usage),*];
            ConstructorWithInput::new(code, v)
        }

    }
}

fn declare_contract_constructor_input_wrapper(constructor: Option<&Constructor>) -> quote::Tokens {
    let constructor_inputs = to_ethabi_param_vec(constructor.iter().flat_map(|c| c.inputs.iter()));

    quote! {
        pub struct ConstructorWithInput {
            encoded_input: ethabi::Bytes,
        }
        impl ethabi::ContractFunction for ConstructorWithInput {
            type Output = ethabi::Address;

            fn encoded(&self) -> ethabi::Bytes {
                self.encoded_input.clone()
            }

            fn output(&self, output_bytes: ethabi::Bytes) -> ethabi::Result<Self::Output> {
                let out = ethabi::decode(&vec![ethabi::ParamType::Address], &output_bytes)?
                    .into_iter()
                    .next()
                    .expect(#INTERNAL_ERR);
                Ok(out.to_address().expect(#INTERNAL_ERR))
            }
        }
        impl ConstructorWithInput {
            pub fn new(code: ethabi::Bytes, tokens: Vec<ethabi::Token>) -> Self {
                let constructor = ethabi::Constructor {
                    inputs: #constructor_inputs
                };

                let encoded_input: ethabi::Bytes = constructor
                    .encode_input(code, &tokens)
                    .expect(#INTERNAL_ERR);

                ConstructorWithInput { encoded_input: encoded_input }
            }
        }

    }
}

fn declare_logs(event: &Event) -> quote::Tokens {
    let name = syn::Ident::from(event.name.to_camel_case());
    let names: Vec<_> = event
        .inputs
        .iter()
        .enumerate()
        .map(|(index, param)| {
            if param.name.is_empty() {
                syn::Ident::from(format!("param{}", index))
            } else {
                param.name.to_snake_case().into()
            }
        })
        .collect();
    let kinds: Vec<_> = event
        .inputs
        .iter()
        .map(|param| rust_type(&param.kind))
        .collect();
    let params: Vec<_> = names
        .iter()
        .zip(kinds.iter())
        .map(|(param_name, kind)| quote! { pub #param_name: #kind, })
        .collect();

    quote! {
        #[derive(Debug, Clone, PartialEq)]
        pub struct #name {
            #(#params)*
        }
    }
}

fn declare_events(event: &Event) -> quote::Tokens {
    let name: syn::Ident = event.name.to_camel_case().into();

    // parse log

    let names: Vec<_> = event
        .inputs
        .iter()
        .enumerate()
        .map(|(index, param)| {
            if param.name.is_empty() {
                if param.indexed {
                    syn::Ident::from(format!("topic{}", index))
                } else {
                    syn::Ident::from(format!("param{}", index))
                }
            } else {
                param.name.to_snake_case().into()
            }
        })
        .collect();

    let log_iter = quote! { log.next().expect(#INTERNAL_ERR).value };

    let to_log: Vec<_> = event
        .inputs
        .iter()
        .map(|param| from_token(&param.kind, &log_iter))
        .collect();

    let log_params: Vec<_> = names
        .iter()
        .zip(to_log.iter())
        .map(|(param_name, convert)| quote! { #param_name: #convert })
        .collect();

    // create filter

    let topic_names: Vec<_> = event
        .inputs
        .iter()
        .enumerate()
        .filter(|&(_, param)| param.indexed)
        .map(|(index, param)| {
            if param.name.is_empty() {
                syn::Ident::from(format!("topic{}", index))
            } else {
                param.name.to_snake_case().into()
            }
        })
        .collect();

    let topic_kinds: Vec<_> = event
        .inputs
        .iter()
        .filter(|param| param.indexed)
        .map(|param| rust_type(&param.kind))
        .collect();

    // [T0, T1, T2]
    let template_names: Vec<_> = get_template_names(&topic_kinds);

    let params: Vec<_> = topic_names
        .iter()
        .zip(template_names.iter())
        .map(|(param_name, template_name)| quote! { #param_name: #template_name })
        .collect();

    // The number of parameters that creates a filter which matches anything.
    let any_params: Vec<_> = params
        .iter()
        .map(|_| quote! { ethabi::Topic::Any })
        .collect();

    let template_params: Vec<_> = topic_kinds
        .iter()
        .zip(template_names.iter())
        .map(|(kind, template_name)| quote! { #template_name: Into<ethabi::Topic<#kind>> })
        .collect();

    let to_filter: Vec<_> = topic_names
        .iter()
        .zip(event.inputs.iter().filter(|p| p.indexed))
        .enumerate()
        .take(3)
        .map(|(index, (param_name, param))| {
            let topic = syn::Ident::from(format!("topic{}", index));
            let i = quote! { i };
            let to_token = to_token(&i, &param.kind);
            quote! { #topic: #param_name.into().map(|#i| #to_token), }
        })
        .collect();

    let event_name = &event.name;

    let event_inputs = &event
        .inputs
        .iter()
        .map(|x| {
            let name = &x.name;
            let kind = to_syntax_string(&x.kind);
            let indexed = x.indexed;

            quote! {
                ethabi::EventParam {
                    name: #name.to_owned(),
                    kind: #kind,
                    indexed: #indexed
                }
            }
        })
        .collect::<Vec<_>>();
    let event_inputs = quote! { vec![ #(#event_inputs),* ] };

    let event_anonymous = &event.anonymous;

    quote! {
        #[derive(Debug, Clone, PartialEq)]
        pub struct #name {
            event: ethabi::Event,
        }

        impl Default for #name {
            fn default() -> Self {
                #name {
                    event: ethabi::Event {
                        name: #event_name.to_owned(),
                        inputs: #event_inputs,
                        anonymous: #event_anonymous
                    }
                }
            }
        }

        impl ParseLog for #name {
            type Log = super::logs::#name;

            /// Parses log.
            fn parse_log(&self, log: ethabi::RawLog) -> ethabi::Result<Self::Log> {
                let mut log = self.event.parse_log(log)?.params.into_iter();
                let result = super::logs::#name {
                    #(#log_params),*
                };
                Ok(result)
            }
        }

        impl LogFilter for #name {
            /// Create a default topic filter that matches any messages.
            fn wildcard_filter(&self) -> ethabi::TopicFilter {
                self.filter(#(#any_params),*)
            }
        }

        impl #name {
            /// Creates topic filter.
            pub fn filter<#(#template_params),*>(&self, #(#params),*) -> ethabi::TopicFilter {
                let raw = ethabi::RawTopicFilter {
                    #(#to_filter)*
                    ..Default::default()
                };

                self.event.filter(raw).expect(#INTERNAL_ERR)
            }
        }
    }
}

fn declare_functions(function: &Function) -> quote::Tokens {
    let name = syn::Ident::from(function.name.to_camel_case());

    let decode_output = {
        let output_kinds = get_output_kinds(&function.outputs);

        let o_impl = match function.outputs.len() {
            0 => quote! { Ok(()) },
            1 => {
                let o = quote! { out };
                let from_first = from_token(&function.outputs[0].kind, &o);
                quote! {
                    let out = self.function.decode_output(output)?.into_iter().next().expect(#INTERNAL_ERR);
                    Ok(#from_first)
                }
            }
            _ => {
                let o = quote! { out.next().expect(#INTERNAL_ERR) };
                let outs: Vec<_> = function
                    .outputs
                    .iter()
                    .map(|param| from_token(&param.kind, &o))
                    .collect();

                quote! {
                    let mut out = self.function.decode_output(output)?.into_iter();
                    Ok(( #(#outs),* ))
                }
            }
        };

        // TODO remove decode_output function for functions without output?
        // Otherwise the output argument is unused
        quote! {
            #[allow(unused_variables)]
            pub fn decode_output(&self, output: &[u8]) -> ethabi::Result<#output_kinds> {
                #o_impl
            }
        }
    };

    let function_name = &function.name;
    let function_inputs = to_ethabi_param_vec(&function.inputs);
    let function_outputs = to_ethabi_param_vec(&function.outputs);
    let function_constant = &function.constant;

    quote! {
        #[derive(Debug, Clone, PartialEq)]
        pub struct #name {
            function: ethabi::Function
        }

        impl Default for #name {
            fn default() -> Self {
                #name {
                    function: ethabi::Function {
                        name: #function_name.to_owned(),
                        inputs: #function_inputs,
                        outputs: #function_outputs,
                        constant: #function_constant
                    }
                }
            }
        }

        impl #name {
            #decode_output

            pub fn encode_input(&self, tokens: &[ethabi::Token]) -> ethabi::Result<ethabi::Bytes> {
                self.function.encode_input(tokens)
            }
        }
    }
}

fn declare_output_functions(function: &Function) -> quote::Tokens {
    let name_camel = syn::Ident::from(function.name.to_camel_case());
    let name_snake = syn::Ident::from(function.name.to_snake_case());
    let output_kinds = get_output_kinds(&function.outputs);

    quote! {
        /// Returns the decoded output for this contract function
        pub fn #name_snake(output_bytes : &[u8]) -> ethabi::Result<#output_kinds> {
            super::functions::#name_camel::default().decode_output(&output_bytes)
        }
    }
}

fn declare_functions_input_wrappers(function: &Function) -> quote::Tokens {
    let name = syn::Ident::from(function.name.to_camel_case());
    let name_with_input = syn::Ident::from(format!("{}WithInput", function.name.to_camel_case()));
    let output_kinds = get_output_kinds(&function.outputs);
    let output_fn_body = quote!{super::functions::#name::default().decode_output(&_output_bytes)};

    quote! {
        /// Contract function with already defined input values
        pub struct #name_with_input {
            encoded_input: ethabi::Bytes
        }

        impl ethabi::ContractFunction for #name_with_input {
            type Output = #output_kinds;

            fn encoded(&self) -> ethabi::Bytes {
                self.encoded_input.clone()
            }

            fn output(&self, _output_bytes: ethabi::Bytes) -> ethabi::Result<Self::Output> {
                #output_fn_body
            }
        }

        impl #name_with_input {
            #[doc(hidden)]
            pub fn new(v: Vec<ethabi::Token>) -> Self {
                let encoded_input : ethabi::Bytes = super::functions::#name::default().encode_input(&v).expect(#INTERNAL_ERR);
                #name_with_input {
                    encoded_input: encoded_input
                }
            }
        }
    }
}

/// Convert input into a rust variable name.
///
/// Avoid using keywords by escaping them.
fn rust_variable(name: &str) -> String {
    // avoid keyword parameters
    match name {
        "self" => "_self".to_string(),
        other => other.to_snake_case(),
    }
}
