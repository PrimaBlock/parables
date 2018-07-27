use ethcore::storage;
use ethereum_types::{Address, H160, H256, U256};
use failure::Error;
use parity_bytes::Bytes;
use serde::de;
use serde_json;
use std::fmt;

macro_rules! ast {
    (
    $(
        $variant:ident {
        $(
            $(#[$($meta:tt)*])*
            $field:ident: $ty:ty,
        )*
        },
    )*
    ) => {
        #[derive(Debug, Deserialize)]
        #[serde(tag = "name")]
        pub enum Ast {
        $(
            #[serde(rename_all = "camelCase")]
            $variant {
                #[serde(default)]
                children: Vec<Ast>,
                src: Src,
                $(
                    $(#[$($meta)*])*
                    $field: $ty,
                )*
            },
        )*
        }

        impl Ast {
            /// Access the kind of the ast.
            pub fn kind(&self) -> &'static str {
                match *self {
                    $(Ast::$variant { ..  } => stringify!($variant),)*
                }
            }

            /// Access the source of this AST element.
            fn source(&self) -> &Src {
                match *self {
                    $(Ast::$variant { ref src, ..  } => src,)*
                }
            }

            /// Access the children of this AST element.
            fn children(&self) -> impl Iterator<Item = &Ast> {
                match *self {
                    $(Ast::$variant { ref children, ..  } => children.iter(),)*
                }
            }
        }
    }
}

pub struct Src {
    start: u32,
    length: u32,
    file_index: u32,
}

impl fmt::Debug for Src {
    fn fmt(&self, fmt: &mut fmt::Formatter) -> fmt::Result {
        write!(
            fmt,
            "\"{}:{}:{}\"",
            self.start, self.length, self.file_index
        )
    }
}

impl<'de> de::Deserialize<'de> for Src {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: de::Deserializer<'de>,
    {
        use serde::de::Error;

        let s = String::deserialize(deserializer)?;

        let mut it = s.split(":");

        let start = it.next()
            .ok_or(D::Error::custom("missing start"))?
            .parse::<u32>()
            .map_err(D::Error::custom)?;

        let length = it.next()
            .ok_or(D::Error::custom("missing length"))?
            .parse::<u32>()
            .map_err(D::Error::custom)?;

        let file_index = it.next()
            .ok_or(D::Error::custom("missing file index"))?
            .parse::<u32>()
            .map_err(D::Error::custom)?;

        Ok(Src {
            start,
            length,
            file_index,
        })
    }
}

#[serde(rename_all = "camelCase")]
#[derive(Debug, Deserialize)]
pub struct FunctionDefinitionAttributes {
    pub name: String,
}

#[serde(rename_all = "camelCase")]
#[derive(Debug, Deserialize)]
pub struct IdentifierAttributes {
    #[serde(rename = "type")]
    pub ty: String,
    pub value: String,
    pub reference_declaration: Option<u32>,
}

#[serde(rename_all = "camelCase")]
#[derive(Debug, Deserialize)]
pub struct IndexAccessAttributes {
    #[serde(rename = "type")]
    pub ty: String,
}

#[serde(rename_all = "camelCase")]
#[derive(Debug, Deserialize)]
pub struct MemberAccessAttributes {
    #[serde(rename = "type")]
    pub ty: String,
    #[serde(rename = "member_name")]
    pub member_name: String,
}

#[serde(rename_all = "camelCase")]
#[derive(Debug, Deserialize)]
pub struct VariableDeclarationAttributes {
    #[serde(rename = "type")]
    pub ty: String,
    pub name: String,
}

#[serde(rename_all = "camelCase")]
#[derive(Debug, Deserialize)]
pub struct AssignmentAttributes {
    #[serde(rename = "type")]
    pub ty: String,
}

#[serde(rename_all = "camelCase")]
#[derive(Debug, Deserialize)]
pub struct ElementaryTypeNameAttributes {
    #[serde(rename = "type")]
    pub ty: String,
    pub name: String,
}

ast!{
    ArrayTypeName { },
    Assignment {
        attributes: AssignmentAttributes,
    },
    BinaryOperation { },
    Block { },
    Conditional { },
    Continue { },
    ContractDefinition { },
    ElementaryTypeName {
        attributes: ElementaryTypeNameAttributes,
    },
    ElementaryTypeNameExpression { },
    EmitStatement { },
    EnumDefinition { },
    EnumValue { },
    EventDefinition { },
    ExpressionStatement { },
    ForStatement { },
    FunctionCall { },
    FunctionDefinition {
        id: u32,
        attributes: FunctionDefinitionAttributes,
    },
    Identifier {
        id: u32,
        attributes: IdentifierAttributes,
    },
    IfStatement { },
    ImportDirective { },
    IndexAccess {
        id: u32,
        attributes: IndexAccessAttributes,
    },
    InheritanceSpecifier { },
    InlineAssembly { },
    Literal { },
    Mapping { },
    MemberAccess {
        id: u32,
        attributes: MemberAccessAttributes,
    },
    ModifierDefinition { },
    ModifierInvocation { },
    NewExpression { },
    ParameterList { },
    PlaceholderStatement { },
    PragmaDirective { },
    Return { },
    SourceUnit { },
    StructDefinition { },
    TupleExpression { },
    UnaryOperation { },
    UserDefinedTypeName { },
    UsingForDirective { },
    VariableDeclaration {
        id: u32,
        attributes: VariableDeclarationAttributes,
    },
    VariableDeclarationStatement { },
}

impl Ast {
    /// Parse AST.
    pub fn parse(input: &str) -> Result<Ast, Error> {
        serde_json::from_str(input).map_err(|e| format_err!("failed to parse AST: {}", e))
    }

    /// Find the first element exactly matching the given span.
    pub fn find(&self, s: u32, l: u32) -> Option<&Ast> {
        let mut current = ::std::collections::VecDeque::new();
        current.push_back(self);

        while let Some(next) = current.pop_front() {
            let Src {
                ref start,
                ref length,
                ..
            } = *next.source();

            if *start == s && *length == l {
                return Some(next);
            }

            current.extend(next.children());
        }

        None
    }

    /// Iterate over all identifiers in the current ast.
    pub fn variables(&self) -> impl Iterator<Item = Variable> {
        use self::Ast::*;

        let mut current = ::std::collections::VecDeque::new();
        current.push_back(self);

        let mut out = Vec::new();

        while let Some(next) = current.pop_front() {
            match *next {
                Identifier { ref attributes, .. } => {
                    out.push(Variable::Identifier {
                        identifier: attributes.value.to_string(),
                    });
                }
                IndexAccess { ref children, .. } => {
                    match children.as_slice() {
                        &[Identifier {
                            attributes: ref key,
                            ..
                        }, Identifier {
                            attributes: ref value,
                            ..
                        }] => {
                            out.push(Variable::IndexAccess {
                                key: key.value.to_string(),
                                value: value.value.to_string(),
                            });
                        }
                        _ => {}
                    }

                    continue;
                }
                MemberAccess {
                    ref attributes,
                    ref children,
                    ..
                } => {
                    match children.as_slice() {
                        &[Identifier {
                            attributes: ref key,
                            ..
                        }] => {
                            out.push(Variable::MemberAccess {
                                key: key.value.to_string(),
                                value: attributes.member_name.to_string(),
                            });
                        }
                        _ => {}
                    }

                    continue;
                }
                _ => {}
            };

            match *next {
                FunctionCall { ref children, .. } => {
                    // first child is function being called.
                    current.extend(children.iter().skip(1));
                }
                ref ast => {
                    current.extend(ast.children());
                }
            }
        }

        out.into_iter()
    }
}

#[derive(Debug)]
pub enum Type {
    BytesCallData,
    BytesMemory,
    Uint256,
    Address,
}

impl Type {
    /// Try to decode the given AST into a type.
    pub fn decode(ty: &str) -> Option<Type> {
        let ty = match ty {
            "bytes calldata" => Type::BytesCallData,
            "bytes memory" => Type::BytesMemory,
            "uint256" => Type::Uint256,
            "address" => Type::Address,
            ty => {
                warn!("Cannot decode type: {}", ty);
                return None;
            }
        };

        Some(ty)
    }

    /// Decode the value from the type, returning the decoded value and the amount of stack that
    /// was used to decode it.
    pub fn value(&self, ctx: &mut Context) -> Result<Value, Error> {
        use self::Type::*;

        match *self {
            BytesCallData => {
                let length = ctx.pop()?.as_u32() as usize;
                let address = ctx.pop()?.as_u32() as usize;

                let data = &ctx.call_data[address..(address + length)];

                Ok(Value::Bytes(data.to_vec()))
            }
            BytesMemory => {
                let address = ctx.pop()?.as_u32() as usize;
                let memory = &ctx.memory[address..];

                let length = U256::from(&memory[..32]).as_u32() as usize;
                let data = &memory[32..(32 + length)];

                Ok(Value::Bytes(data.to_vec()))
            }
            Uint256 => {
                let value = ctx.pop()?;

                Ok(Value::Uint256(value))
            }
            Address => {
                let value = ctx.pop()?;
                let address = self::Address::from(H160::from(H256::from(value)));
                Ok(Value::Address(address))
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Value {
    Bytes(Vec<u8>),
    Uint256(U256),
    Address(Address),
}

impl fmt::Display for Value {
    fn fmt(&self, fmt: &mut fmt::Formatter) -> fmt::Result {
        use self::Value::*;

        match *self {
            Bytes(ref bytes) => write!(fmt, "bytes({:?})", bytes),
            Uint256(ref value) => write!(fmt, "uint256({})", value),
            Address(ref value) => write!(fmt, "address({})", value),
        }
    }
}

#[derive(Clone, Copy)]
pub struct Context<'a> {
    stack: &'a [U256],
    memory: &'a [u8],
    #[allow(unused)]
    storage: &'a storage::StorageAccess,
    call_data: &'a Bytes,
}

impl<'a> Context<'a> {
    /// Create a new variable context.
    pub fn new(
        stack: &'a [U256],
        memory: &'a [u8],
        storage: &'a storage::StorageAccess,
        call_data: &'a Bytes,
    ) -> Context<'a> {
        Context {
            stack,
            memory,
            storage,
            call_data,
        }
    }

    /// Pop a value from the stack.
    pub fn pop(&mut self) -> Result<U256, Error> {
        if self.stack.is_empty() {
            bail!("expected element on stack, but was empty");
        }

        let out = self.stack[self.stack.len() - 1];
        self.stack = &self.stack[..self.stack.len() - 1];
        Ok(out)
    }
}

impl fmt::Display for Type {
    fn fmt(&self, fmt: &mut fmt::Formatter) -> fmt::Result {
        use self::Type::*;

        match *self {
            BytesCallData => write!(fmt, "bytes calldata"),
            BytesMemory => write!(fmt, "bytes memory"),
            Uint256 => write!(fmt, "uint256"),
            Address => write!(fmt, "address"),
        }
    }
}

#[derive(Debug, Clone, Hash, PartialEq, Eq)]
pub enum Variable {
    /// identifier expressions
    Identifier { identifier: String },
    /// key[value] expressions
    IndexAccess { key: String, value: String },
    /// key.value expressions
    MemberAccess { key: String, value: String },
}

impl fmt::Display for Variable {
    fn fmt(&self, fmt: &mut fmt::Formatter) -> fmt::Result {
        use self::Variable::*;

        match *self {
            Identifier { ref identifier } => identifier.fmt(fmt),
            IndexAccess { ref key, ref value } => write!(fmt, "{}[{}]", key, value),
            MemberAccess { ref key, ref value } => write!(fmt, "{}.{}", key, value),
        }
    }
}
