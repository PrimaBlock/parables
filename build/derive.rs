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
    source_map: Option<String>,
    #[serde(rename = "bin-runtime")]
    runtime_bin: Option<String>,
    #[serde(rename = "srcmap-runtime")]
    runtime_source_map: Option<String>,
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

#[derive(Debug, Hash, PartialEq, Eq)]
pub struct Name {
    path: String,
    module_name: String,
    type_module_name: String,
    type_name: String,
}

impl fmt::Display for Name {
    fn fmt(&self, fmt: &mut fmt::Formatter) -> fmt::Result {
        write!(fmt, "{}:{}", self.path, self.type_name)
    }
}

/// Implement a module for the given output.
pub fn impl_module(path: &Path, output: Output) -> Result<quote::Tokens> {
    let mut result = Vec::new();

    let mut map = HashMap::new();

    for (name, contract) in output.contracts {
        let name = parse_name(&name)?;

        map.entry(name.module_name.to_string())
            .or_insert_with(Vec::new)
            .push((name, contract));
    }

    for (module_name, values) in map.into_iter() {
        let module_name = syn::Ident::from(module_name.as_str());

        let mut types = Vec::new();

        for (name, contract) in values {
            let contract = impl_contract_abi(&name, &contract, &contract.abi)?;

            let type_module_name = syn::Ident::from(name.type_module_name.as_str());

            types.push(quote! {
                pub mod #type_module_name {
                    #contract
                }
            });
        }

        result.push(quote! {
            pub mod #module_name {
                #(#types)*
            }
        });
    }

    result.push(new_context_function(path, output.source_list));

    return Ok(quote!{ #(#result)* });

    fn parse_name(name: &str) -> Result<Name> {
        let mut parts = name.split(":");

        let path = parts.next().ok_or_else(|| format!("bad name: {}", name))?;
        let type_name = parts.next().ok_or_else(|| format!("bad name: {}", name))?;

        let mut parts = path.split(".");
        let base = parts.next().ok_or_else(|| format!("bad path: {}", path))?;

        let module_name = base.to_snake_case();
        let type_module_name = type_name.to_snake_case();

        Ok(Name {
            path: path.to_string(),
            module_name,
            type_module_name,
            type_name: type_name.to_string(),
        })
    }

    fn new_context_function(path: &Path, source_list: Vec<String>) -> quote::Tokens {
        let source_list = source_list
            .into_iter()
            .map(|p| path.join(p).display().to_string())
            .collect::<Vec<_>>();

        quote! {
            pub fn new_context() -> ::parables_testing::abi::ContractContext {
                ::parables_testing::abi::ContractContext {
                    source_list: Some(vec![#(::std::path::Path::new(#source_list).to_owned(),)*]),
                }
            }
        }
    }
}

/// Implement the contract ABI.
fn impl_contract_abi(
    name: &Name,
    contract_fields: &ContractFields,
    input: &str,
) -> Result<quote::Tokens> {
    let contract: Contract = serde_json::from_str(input)?;

    let mut static_functions = Vec::new();
    let mut impl_functions = Vec::new();
    let mut func_structs = Vec::new();
    let mut output_functions = Vec::new();
    let mut func_input_wrappers_structs = Vec::new();

    for f in contract.functions() {
        let (static_function, impl_function) = impl_contract_function(f);

        static_functions.push(static_function);
        impl_functions.push(impl_function);
        func_structs.push(declare_functions(f));
        output_functions.push(declare_output_functions(f));
        func_input_wrappers_structs.push(declare_functions_input_wrappers(f));
    }

    let events_impl: Vec<_> = contract.events().map(impl_contract_event).collect();
    let constructor_impl = impl_constructor(name, contract_fields, contract.constructor.as_ref());
    let logs_structs: Vec<_> = contract.events().map(declare_logs).collect();
    let events_structs: Vec<_> = contract.events().map(declare_events).collect();

    let events_and_logs_quote = if events_structs.is_empty() {
        quote!{}
    } else {
        quote! {
            pub mod events {
                #[allow(unused)]
                use parables_testing::ethabi;

                #(#events_structs)*

                #(#events_impl)*
            }

            pub mod logs {
                #[allow(unused)]
                use parables_testing::ethabi;

                #(#logs_structs)*
            }
        }
    };

    let wrapper_quote = impl_wrapper(impl_functions);

    let functions_quote = if func_structs.is_empty() {
        quote!{}
    } else {
        quote! {
            pub mod functions {
                #[allow(unused)]
                use parables_testing::ethabi;

                #(#func_structs)*

                #(#static_functions)*

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
                #[allow(unused)]
                use parables_testing::ethabi;

                #(#output_functions)*
            }
        }
    };

    let result = quote! {
        #[allow(unused)]
        use parables_testing::ethabi;

        #constructor_impl

        #events_and_logs_quote

        #outputs_quote

        #wrapper_quote

        #functions_quote
    };

    return Ok(result);

    fn impl_wrapper(impl_functions: Vec<quote::Tokens>) -> quote::Tokens {
        quote! {
            #[allow(unused)]
            pub struct Contract<'a, VM: 'a> {
                vm: &'a VM,
                pub address: ethabi::Address,
                call: ::parables_testing::call::Call,
            }

            impl<'a, VM> Clone for Contract<'a, VM> {
                fn clone(&self) -> Self {
                    *self
                }
            }

            impl<'a, VM> Copy for Contract<'a, VM> {
            }

            impl<'a, VM> Contract<'a, VM> {
                #(#impl_functions)*

                /// Modify the call for the contract.
                pub fn call(self, call: ::parables_testing::call::Call) -> Self {
                    Self {
                        call: call,
                        ..self
                    }
                }

                /// Modify the default sender for the contract.
                pub fn sender(self, sender: ethabi::Address) -> Self {
                    Self {
                        call: self.call.sender(sender),
                        ..self
                    }
                }

                /// Modify the default value for a copy of the current contract.
                pub fn value<V>(self, value: V) -> Self
                    where V: Into<::parables_testing::ethereum_types::U256>
                {
                    Self {
                        call: self.call.value(value),
                        ..self
                    }
                }
            }

            pub fn contract<'a, VM>(
                vm: &'a VM,
                address: ethabi::Address,
                call: ::parables_testing::call::Call
            ) -> Contract<'a, VM>
                where VM: ::parables_testing::abi::Vm
            {
                Contract { vm, address, call }
            }
        }
    }
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

fn impl_contract_function(function: &Function) -> (quote::Tokens, quote::Tokens) {
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

    // [param0, hello_world, param2]
    let ref param_names: Vec<_> = input_names
        .iter()
        .zip(template_names.iter())
        .map(|(param_name, _)| quote! { #param_name })
        .collect();

    // [Token::Uint(param0.into()), Token::Bytes(hello_world.into()), Token::Array(param2.into_iter().map(Into::into).collect())]
    let usage: Vec<_> = input_names
        .iter()
        .zip(function.inputs.iter())
        .map(|(param_name, param)| {
            to_token(&from_template_param(&param.kind, &param_name), &param.kind)
        })
        .collect();

    let output_kinds = get_output_kinds(&function.outputs);

    let name = syn::Ident::from(function.name.to_snake_case());

    let static_function = quote! {
        /// Sets the input (arguments) for this contract function
        pub fn #name<#(#template_params),*>(#(#params),*) -> #function_input_wrapper_name {
            let v: Vec<ethabi::Token> = vec![#(#usage),*];
            #function_input_wrapper_name::new(v)
        }
    };

    let impl_function_name = match function.name.as_str() {
        "address" => syn::Ident::from("_address"),
        "sender" => syn::Ident::from("_sender"),
        "value" => syn::Ident::from("_value"),
        "gas" => syn::Ident::from("_gas"),
        "gas_price" => syn::Ident::from("_gas_price"),
        value => syn::Ident::from(value.to_snake_case()),
    };

    let impl_function = quote! {
        /// Sets the input (arguments) for this contract function
        pub fn #impl_function_name<#(#template_params),*>(&self, #(#params),*)
            -> ::std::result::Result<
                ::parables_testing::evm::CallOutput<#output_kinds>,
                ::parables_testing::error::CallError<::parables_testing::evm::CallResult>
            >
            where VM: ::parables_testing::abi::Vm
        {
            let function_call = self::functions::#name(#(#param_names),*);
            self.vm.call(self.address, function_call, self.call)
        }
    };

    (static_function, impl_function)
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

fn impl_constructor(
    name: &Name,
    contract_fields: &ContractFields,
    constructor: Option<&Constructor>,
) -> quote::Tokens {
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

    let constructor_inputs = to_ethabi_param_vec(constructor.iter().flat_map(|c| c.inputs.iter()));

    let item = &name.type_name;
    let bin = &contract_fields.bin;

    let source_map = match contract_fields.source_map.as_ref() {
        Some(source_map) => quote!{ Some(#source_map) },
        None => quote!{ None },
    };

    let runtime_bin = match contract_fields.runtime_bin.as_ref() {
        Some(runtime_bin) => quote!{ Some(#runtime_bin) },
        None => quote!{ None },
    };

    let runtime_source_map = match contract_fields.runtime_source_map.as_ref() {
        Some(runtime_source_map) => quote!{ Some(#runtime_source_map) },
        None => quote!{ None },
    };

    quote! {
        pub fn constructor<#(#template_params),*>(#(#params),* ) -> Constructor {
            let v: Vec<ethabi::Token> = vec![#(#usage),*];
            Constructor::new(v)
        }

        pub struct Constructor {
            tokens: Vec<ethabi::Token>,
        }

        impl Constructor {
            pub fn new(tokens: Vec<ethabi::Token>) -> Self {
                Constructor { tokens }
            }
        }

        impl ::parables_testing::abi::ContractFunction for Constructor {
            type Output = ethabi::Address;

            fn encoded(&self, linker: &::parables_testing::linker::Linker)
                -> ::std::result::Result<ethabi::Bytes, ::parables_testing::error::Error>
            {
                let constructor = ethabi::Constructor {
                    inputs: #constructor_inputs
                };

                let code = linker.link(<Self as ::parables_testing::abi::Constructor>::BIN)?;

                let encoded: ethabi::Bytes = constructor
                    .encode_input(code, &self.tokens)
                    .expect(#INTERNAL_ERR);

                Ok(encoded)
            }

            fn output(&self, output_bytes: ethabi::Bytes)
                -> ::std::result::Result<ethabi::Address, ::parables_testing::error::Error>
            {
                let out = ethabi::decode(&vec![ethabi::ParamType::Address], &output_bytes)
                    .map_err(|e| format!("failed to decode output: {}", e))?;

                let out = out.into_iter().next()
                    .ok_or_else(|| "expected one parameter")?;

                let out = out.to_address()
                    .ok_or_else(|| "failed to convert output to address")?;

                Ok(out)
            }
        }

        impl ::parables_testing::abi::Constructor for Constructor {
            const ITEM: &'static str = #item;
            const BIN: &'static str = #bin;
            const SOURCE_MAP: Option<&'static str> = #source_map;
            const RUNTIME_BIN: Option<&'static str> = #runtime_bin;
            const RUNTIME_SOURCE_MAP: Option<&'static str> = #runtime_source_map;
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

    let parse_log = match log_params.len() {
        0 => quote! {
            /// Parses log.
            fn parse_log(&self, _log: ethabi::RawLog)
                -> ::std::result::Result<Self::Log, ::parables_testing::error::Error>
            {
                Ok(super::logs::#name { })
            }
        },
        _ => quote! {
            /// Parses log.
            fn parse_log(&self, log: ethabi::RawLog)
                -> ::std::result::Result<Self::Log, ::parables_testing::error::Error>
            {
                let log = self.event.parse_log(log)
                    .map_err(|e| format!("failed to parse log: {}", e))?;

                let mut log = log.params.into_iter();

                Ok(super::logs::#name {
                    #(#log_params),*
                })
            }
        },
    };

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

        impl ::parables_testing::abi::ParseLog for #name {
            type Log = super::logs::#name;

            #parse_log
        }

        impl ::parables_testing::abi::LogFilter for #name {
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
                    let out = self.function.decode_output(output)
                        .map_err(|e| format!("failed to decode output: {}", e))?;

                    let out = out.into_iter().next()
                        .ok_or_else(|| "expected one parameter")?;

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
                    let mut out = self.function.decode_output(output)
                        .map_err(|e| format!("failed to decode output: {}", e))?
                        .into_iter();

                    Ok(( #(#outs),* ))
                }
            }
        };

        // TODO remove decode_output function for functions without output?
        // Otherwise the output argument is unused
        quote! {
            #[allow(unused_variables)]
            pub fn decode_output(&self, output: &[u8])
                -> ::std::result::Result<#output_kinds, ::parables_testing::error::Error>
            {
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

            pub fn encode_input(&self, tokens: &[ethabi::Token])
                -> ::std::result::Result<ethabi::Bytes, ::parables_testing::error::Error>
            {
                self.function.encode_input(tokens)
                    .map_err(|e| format!("failed to encode input: {}", e).into())
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
        pub fn #name_snake(output_bytes : &[u8])
            -> ::std::result::Result<#output_kinds, ::parables_testing::error::Error>
        {
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

        impl ::parables_testing::abi::ContractFunction for #name_with_input {
            type Output = #output_kinds;

            fn encoded(&self, _linker: &::parables_testing::linker::Linker)
                -> ::std::result::Result<ethabi::Bytes, ::parables_testing::error::Error>
            {
                Ok(self.encoded_input.clone())
            }

            fn output(&self, _output_bytes: ethabi::Bytes)
                -> ::std::result::Result<Self::Output, ::parables_testing::error::Error>
            {
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
