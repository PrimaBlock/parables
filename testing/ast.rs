use ethereum_types::{Address, H160, H256, U256};
use failure::Error;
use parity_bytes::Bytes;
use serde::de;
use serde_json;
use source_map;
use std::collections::{hash_map, BTreeMap, HashMap, HashSet};
use std::fmt;
use std::sync::Arc;

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
                children: Vec<Arc<Ast>>,
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
            pub fn source(&self) -> &Src {
                match *self {
                    $(Ast::$variant { ref src, ..  } => src,)*
                }
            }

            /// Access the children by ref-counter AST element.
            fn children<'a>(&'a self) -> impl Iterator<Item = &'a Arc<Ast>> {
                match *self {
                    $(Ast::$variant { ref children, ..  } => children.iter(),)*
                }
            }
        }
    }
}

#[derive(Clone, Copy, Hash, PartialEq, Eq, PartialOrd, Ord)]
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

        let start = it
            .next()
            .ok_or(D::Error::custom("missing start"))?
            .parse::<u32>()
            .map_err(D::Error::custom)?;

        let length = it
            .next()
            .ok_or(D::Error::custom("missing length"))?
            .parse::<u32>()
            .map_err(D::Error::custom)?;

        let file_index = it
            .next()
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
pub struct FunctionCallAttributes {
    #[serde(rename = "type")]
    pub ty: String,
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
    pub referenced_declaration: u32,
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
    pub is_pure: bool,
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

#[serde(rename_all = "camelCase")]
#[derive(Debug, Deserialize)]
pub struct ElementaryTypeNameExpressionAttributes {
    #[serde(rename = "type")]
    pub ty: String,
    pub value: String,
}

#[serde(rename_all = "camelCase")]
#[derive(Debug, Deserialize)]
pub struct EnumDefinitionAttributes {
    pub canonical_name: String,
    pub name: String,
}

#[serde(rename_all = "camelCase")]
#[derive(Debug, Deserialize)]
pub struct EnumValueAttributes {
    pub name: String,
}

ast!{
    ArrayTypeName { },
    Assignment {
        attributes: AssignmentAttributes,
    },
    BinaryOperation { },
    Block { },
    Break { },
    Conditional { },
    Continue { },
    ContractDefinition { },
    ElementaryTypeName {
        attributes: ElementaryTypeNameAttributes,
    },
    ElementaryTypeNameExpression {
        attributes: ElementaryTypeNameExpressionAttributes,
    },
    EmitStatement { },
    EnumDefinition {
        id: u32,
        attributes: EnumDefinitionAttributes,
    },
    EnumValue {
        id: u32,
        attributes: EnumValueAttributes,
    },
    EventDefinition { },
    ExpressionStatement { },
    ForStatement { },
    FunctionCall {
        attributes: FunctionCallAttributes,
    },
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

#[derive(Debug)]
pub struct Function {
    pub src: Src,
    pub name: String,
}

#[derive(Debug, Clone, Hash, PartialEq, Eq, PartialOrd, Ord)]
pub struct Variant {
    pub name: String,
}

#[derive(Debug, Clone, Hash, PartialEq, Eq, PartialOrd, Ord)]
pub struct Enum {
    pub name: String,
    pub variants: Vec<Variant>,
}

#[derive(Debug, Default)]
pub struct Registry {
    /// ASTs indexed by source location.
    index: HashMap<(u32, u32), Arc<Ast>>,
    /// Set of statements.
    statements: HashSet<Src>,
    /// Ranges of functions.
    functions: HashMap<u32, BTreeMap<u32, Arc<Function>>>,
    /// Enums, to lookup variant names.
    enums: HashMap<String, Arc<Enum>>,
}

impl Registry {
    /// Parse AST.
    pub fn parse(input: &str) -> Result<Registry, Error> {
        let ast: Ast =
            serde_json::from_str(input).map_err(|e| format_err!("failed to parse AST: {}", e))?;

        let ast = Arc::new(ast);

        let mut index = HashMap::new();
        let mut statements = HashSet::new();
        // mapping location ranges to functions.
        let mut functions = HashMap::new();
        // mapping from enum variants to struct to figure out name.
        let mut enums = HashMap::new();

        let mut current = ::std::collections::VecDeque::new();
        current.push_back(&ast);

        while let Some(next) = current.pop_front() {
            let src = next.source();
            let key = (src.start, src.length);

            match next.as_ref() {
                Ast::FunctionDefinition { ref attributes, .. } => {
                    let function = Arc::new(Function {
                        src: src.clone(),
                        name: attributes.name.to_string(),
                    });

                    functions
                        .entry(src.file_index)
                        .or_insert_with(BTreeMap::new)
                        .insert(src.start, function);
                }
                Ast::EnumDefinition {
                    ref attributes,
                    ref children,
                    ..
                } => {
                    let mut variants = Vec::new();

                    for c in children {
                        if let Ast::EnumValue { ref attributes, .. } = *c.as_ref() {
                            variants.push(Variant {
                                name: attributes.name.to_string(),
                            });
                        }
                    }

                    let enum_ = Arc::new(Enum {
                        name: attributes.name.to_string(),
                        variants: variants,
                    });

                    enums.insert(attributes.canonical_name.to_string(), enum_);
                }
                _ => {}
            }

            if let hash_map::Entry::Vacant(e) = index.entry(key) {
                statements.insert(next.source().clone());
                e.insert(Arc::clone(next));
            }

            current.extend(next.children());
        }

        Ok(Registry {
            index,
            statements,
            functions,
            enums,
        })
    }

    /// Find the function the encapsulated the specified mapping.
    pub fn find_function(&self, mapping: &source_map::Mapping) -> Option<&Arc<Function>> {
        use std::ops::Bound;

        let file_index = match mapping.file_index {
            Some(file_index) => file_index,
            None => return None,
        };

        let functions = match self.functions.get(&file_index) {
            Some(function) => function,
            None => return None,
        };

        let mut it = functions.range((Bound::Unbounded, Bound::Included(mapping.start)));

        if let Some((_, f)) = it.next_back() {
            let end = f.src.start + f.src.length;
            let lookup_end = mapping.start + mapping.length;

            if lookup_end <= end {
                return Some(f);
            }
        }

        None
    }

    /// Find the first element exactly matching the given span.
    pub fn find(&self, mapping: &source_map::Mapping) -> Option<&Ast> {
        let src = (mapping.start, mapping.length);
        self.index.get(&src).map(|a| a.as_ref())
    }

    /// Find the location of all statements in registry.
    pub fn statements(&self) -> impl Iterator<Item = &Src> {
        self.statements.iter()
    }

    /// Decode AST into an expression.
    /// If AST cannot be decoded, returns `None`.
    pub fn decode_ast<'a>(&self, c: &'a Ast) -> Option<(Expr, &'a str)> {
        use self::Ast::*;

        match *c {
            Identifier { ref attributes, .. } => {
                let var = Expr::Identifier {
                    identifier: attributes.value.to_string(),
                };

                return Some((var, attributes.ty.as_str()));
            }
            ElementaryTypeNameExpression { ref attributes, .. } => {
                let var = Expr::Identifier {
                    identifier: attributes.value.to_string(),
                };

                return Some((var, attributes.ty.as_str()));
            }
            Assignment { ref children, .. } => {
                let mut it = children.iter().map(|a| a.as_ref());

                match it.next() {
                    Some(Identifier { ref attributes, .. }) => {
                        let var = Expr::Identifier {
                            identifier: attributes.value.to_string(),
                        };

                        Some((var, attributes.ty.as_str()))
                    }
                    _ => None,
                }
            }
            IndexAccess {
                ref attributes,
                ref children,
                ..
            } => {
                let mut it = children.iter().map(|a| a.as_ref());

                let key = self.decode_ast(it.next()?)?.0;
                let value = self.decode_ast(it.next()?)?.0;

                let var = Expr::IndexAccess {
                    key: Box::new(key),
                    value: Box::new(value),
                };

                Some((var, attributes.ty.as_str()))
            }
            MemberAccess {
                ref attributes,
                ref children,
                ..
            } => {
                // Pure member accesses can be understood without decoding.
                // This is typically enum values, like `State.Open`.
                if attributes.is_pure {
                    return None;
                }

                let mut it = children.iter().map(|a| a.as_ref());

                let key = self.decode_ast(it.next()?)?.0;

                let var = Expr::MemberAccess {
                    key: Box::new(key),
                    value: attributes.member_name.to_string(),
                };

                Some((var, attributes.ty.as_str()))
            }
            FunctionCall {
                ref attributes,
                ref children,
                ..
            } => {
                let mut it = children.iter().map(|a| a.as_ref());
                let name = self.decode_ast(it.next()?)?.0;
                let mut args = Vec::new();

                for c in it {
                    args.push(self.decode_ast(c)?.0);
                }

                let var = Expr::FunctionCall {
                    name: Box::new(name),
                    args,
                };

                Some((var, attributes.ty.as_str()))
            }
            _ => None,
        }
    }

    /// Try to decode the given AST into a type.
    pub fn decode_type(&self, ty: &str) -> Type {
        use self::Type::*;

        if ty.starts_with("mapping") {
            let mapping = &ty[7..];

            if &mapping[..1] != "(" || &mapping[mapping.len() - 1..] != ")" {
                return Type::Unknown(ty.into());
            }

            let mapping = &mapping[1..mapping.len() - 1];

            let mut it = mapping.split("=>");

            let from = match it.next() {
                Some(from) => from.trim(),
                None => return Type::Unknown(ty.into()),
            };

            let to = match it.next() {
                Some(to) => to.trim(),
                None => return Type::Unknown(ty.into()),
            };

            let from = self.decode_type(from);
            let to = self.decode_type(to);

            return Type::Mapping(Box::new(from), Box::new(to));
        }

        let mut it = ty.split(" ");

        match it.next() {
            Some("enum") => {
                let name = match it.next() {
                    Some(name) => name,
                    _ => return Unknown(ty.into()),
                };

                if let Some(enum_) = self.enums.get(name) {
                    return Enum(name.to_string(), Some(Arc::clone(enum_)));
                }

                return Enum(name.to_string(), None);
            }
            Some("struct") => {
                let name = match it.next() {
                    Some(name) => name,
                    _ => return Unknown(ty.into()),
                };

                let storage = match it.next() {
                    Some("storage") => Storage::Storage,
                    Some("memory") => Storage::Memory,
                    Some("calldata") => Storage::CallData,
                    _ => return Unknown(ty.into()),
                };

                let kind = match it.next() {
                    Some("pointer") => Kind::Pointer,
                    Some("ref") => Kind::Ref,
                    _ => return Unknown(ty.into()),
                };

                return Struct(name.to_string(), storage, kind);
            }
            Some("function") => {
                let mut it = ty.splitn(2, " ");

                if it.next() != Some("function") {
                    return Unknown(ty.into());
                }

                let params = match it.next() {
                    Some(params) => params,
                    _ => return Unknown(ty.into()),
                };

                return Function(params.to_string());
            }
            Some("bytes") => match it.next() {
                Some("calldata") => Bytes(Storage::CallData),
                Some("memory") => Bytes(Storage::Memory),
                _ => return Unknown(ty.into()),
            },
            Some("bytes32") => Bytes32,
            Some("uint256") => Uint256,
            Some("bool") => Bool,
            Some("address") => Address,
            _ => return Unknown(ty.into()),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Kind {
    /// A pointer.
    Pointer,
    /// A reference.
    Ref,
}

impl fmt::Display for Kind {
    fn fmt(&self, fmt: &mut fmt::Formatter) -> fmt::Result {
        use self::Kind::*;

        match *self {
            Pointer => write!(fmt, "pointer"),
            Ref => write!(fmt, "ref"),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Storage {
    /// Something stored in storage.
    Storage,
    /// Something stored in memory.
    Memory,
    /// Something stored in calldata.
    CallData,
}

impl fmt::Display for Storage {
    fn fmt(&self, fmt: &mut fmt::Formatter) -> fmt::Result {
        use self::Storage::*;

        match *self {
            Storage => write!(fmt, "storage"),
            Memory => write!(fmt, "memory"),
            CallData => write!(fmt, "calldata"),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Type {
    Bytes(Storage),
    Bytes32,
    Uint256,
    Bool,
    Address,
    Enum(String, Option<Arc<Enum>>),
    Struct(String, Storage, Kind),
    Function(String),
    Mapping(Box<Type>, Box<Type>),
    Unknown(String),
}

impl Type {
    /// Decode the value from the type, returning the decoded value and the amount of stack that
    /// was used to decode it.
    pub fn value(self, ctx: &mut Context) -> Result<Value, Error> {
        use self::Type::*;

        match self {
            Bytes(Storage::CallData) => {
                let length = ctx.pop()?.as_u32() as usize;
                let address = ctx.pop()?.as_u32() as usize;

                let data = &ctx.call_data[address..(address + length)];

                Ok(Value::Bytes(data.to_vec()))
            }
            Bytes(Storage::Memory) => {
                let address = ctx.pop()?.as_u32() as usize;
                let memory = &ctx.memory[address..];

                let length = U256::from(&memory[..32]).as_u32() as usize;
                let data = &memory[32..(32 + length)];

                Ok(Value::Bytes(data.to_vec()))
            }
            Bytes32 => {
                let value = ctx.pop()?;
                Ok(Value::Bytes32(<[u8; 32]>::from(value)))
            }
            Uint256 => {
                let value = ctx.pop()?;
                Ok(Value::Uint256(value))
            }
            Bool => {
                let value = ctx.pop()?;
                Ok(Value::Bool(value != U256::from(0)))
            }
            Address => {
                let value = ctx.pop()?;
                let address = self::Address::from(H160::from(H256::from(value)));
                Ok(Value::Address(address))
            }
            Mapping(key, value) => Ok(Value::Mapping(*key, *value)),
            Enum(name, enum_) => {
                let value = ctx.pop()?;
                let variant = enum_.as_ref().and_then(|enum_| {
                    enum_
                        .variants
                        .get(value.as_usize())
                        .map(|v| v.name.to_string())
                });

                Ok(Value::Enum {
                    name,
                    value,
                    variant,
                })
            }
            Struct(name, storage, kind) => Ok(Value::Struct(name, storage, kind)),
            Function(params) => Ok(Value::Function(params)),
            Unknown(ty) => Ok(Value::Unknown(ty)),
            _ => Ok(Value::Unknown("unknown".into())),
        }
    }
}

impl fmt::Display for Type {
    fn fmt(&self, fmt: &mut fmt::Formatter) -> fmt::Result {
        use self::Type::*;

        match *self {
            Bytes(ref storage) => write!(fmt, "bytes {}", storage),
            Bytes32 => write!(fmt, "bytes32"),
            Uint256 => write!(fmt, "uint256"),
            Bool => write!(fmt, "bool"),
            Address => write!(fmt, "address"),
            Mapping(ref key, ref value) => write!(fmt, "mapping({} => {})", key, value),
            Enum(ref name, ..) => write!(fmt, "enum {}", name),
            Struct(ref name, ref storage, ref kind) => {
                write!(fmt, "struct {} {} {}", name, storage, kind)
            }
            Function(ref params) => write!(fmt, "function {}", params),
            Unknown(ref ty) => write!(fmt, "{}", ty),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Value {
    /// A byte-array.
    Bytes(Vec<u8>),
    /// A bytes32 value.
    Bytes32([u8; 32]),
    /// A uint256
    Uint256(U256),
    /// A bool.
    Bool(bool),
    /// An address.
    Address(Address),
    /// Only store the types of a mapping since we can't discover all values.
    Mapping(Type, Type),
    /// An enum value.
    Enum {
        name: String,
        value: U256,
        variant: Option<String>,
    },
    /// A struct and its name.
    Struct(String, Storage, Kind),
    /// A function.
    Function(String),
    Unknown(String),
}

impl fmt::Display for Value {
    fn fmt(&self, fmt: &mut fmt::Formatter) -> fmt::Result {
        use self::Value::*;

        match *self {
            Bytes(ref bytes) => write!(fmt, "bytes({}, {})", Hex(bytes), bytes.len()),
            Bytes32(ref value) => write!(fmt, "bytes32({})", Hex(value)),
            Uint256(ref value) => write!(fmt, "uint256({})", value),
            Bool(ref value) => write!(fmt, "bool({})", value),
            Address(ref value) => write!(fmt, "address({:?})", value),
            Mapping(ref key, ref value) => write!(fmt, "mapping({} => {})", key, value),
            Enum {
                ref name,
                ref value,
                ref variant,
                ..
            } => {
                if let Some(variant) = variant.as_ref() {
                    write!(fmt, "enum {}.{}", name, variant)
                } else {
                    write!(fmt, "enum {}({})", name, value)
                }
            }
            Struct(ref name, ref storage, ref kind) => {
                write!(fmt, "struct {} {} {}", name, storage, kind)
            }
            Function(ref params) => write!(fmt, "function {}", params),
            Unknown(ref ty) => write!(fmt, "{}(*unknown*)", ty),
        }
    }
}

#[derive(Clone, Copy)]
pub struct Context<'a> {
    stack: &'a [U256],
    memory: &'a [u8],
    call_data: &'a Bytes,
}

impl<'a> Context<'a> {
    /// Create a new decoding context.
    pub fn new(stack: &'a [U256], memory: &'a [u8], call_data: &'a Bytes) -> Context<'a> {
        Context {
            stack,
            memory,
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

#[derive(Debug, Clone, Hash, PartialEq, Eq, PartialOrd, Ord)]
pub enum Expr {
    /// identifier expressions
    Identifier { identifier: String },
    /// key[value] expressions
    IndexAccess { key: Box<Expr>, value: Box<Expr> },
    /// key.value expressions
    MemberAccess { key: Box<Expr>, value: String },
    /// name(args) function calls.
    FunctionCall { name: Box<Expr>, args: Vec<Expr> },
}

impl fmt::Display for Expr {
    fn fmt(&self, fmt: &mut fmt::Formatter) -> fmt::Result {
        use self::Expr::*;

        match *self {
            Identifier { ref identifier } => identifier.fmt(fmt),
            IndexAccess { ref key, ref value } => write!(fmt, "{}[{}]", key, value),
            MemberAccess { ref key, ref value } => write!(fmt, "{}.{}", key, value),
            FunctionCall { ref name, ref args } => {
                let args = args
                    .iter()
                    .map(|a| a.to_string())
                    .collect::<Vec<String>>()
                    .join(", ");
                write!(fmt, "{}({})", name, args)
            }
        }
    }
}

struct Hex<'a>(&'a [u8]);

impl<'a> fmt::Display for Hex<'a> {
    fn fmt(&self, fmt: &mut fmt::Formatter) -> fmt::Result {
        write!(fmt, "0x")?;

        for b in self.0 {
            write!(fmt, "{:02x}", b)?;
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::{Kind, Registry, Storage, Type};

    #[test]
    fn test_decode() {
        let registry = Registry::default();

        assert_eq!(Type::Uint256, registry.decode_type("uint256"));
        assert_eq!(Type::Address, registry.decode_type("address"));
        assert_eq!(
            Type::Bytes(Storage::CallData),
            registry.decode_type("bytes calldata")
        );

        assert_eq!(
            Type::Mapping(Box::new(Type::Address), Box::new(Type::Uint256)),
            registry.decode_type("mapping(address => uint256)")
        );

        assert_eq!(
            Type::Struct("Foo".to_string(), Storage::Storage, Kind::Pointer),
            registry.decode_type("struct Foo storage pointer")
        );

        assert_eq!(
            Type::Function("(uint256)".to_string()),
            registry.decode_type("function (uint256)")
        );
    }
}
