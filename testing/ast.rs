use failure::Error;
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
pub struct IdentifierAttributes {
    #[serde(rename = "type")]
    pub ty: String,
    pub value: String,
    pub reference_declaration: Option<u32>,
}

#[serde(rename_all = "camelCase")]
#[derive(Debug, Deserialize)]
pub struct VariableDeclarationAttributes {
    #[serde(rename = "type")]
    pub ty: String,
    pub name: String,
}

ast!{
    ArrayTypeName { },
    Assignment { },
    BinaryOperation { },
    Block { },
    Conditional { },
    Continue { },
    ContractDefinition { },
    ElementaryTypeName { },
    ElementaryTypeNameExpression { },
    EmitStatement { },
    EnumDefinition { },
    EnumValue { },
    EventDefinition { },
    ExpressionStatement { },
    ForStatement { },
    FunctionCall { },
    FunctionDefinition { },
    Identifier {
        attributes: IdentifierAttributes,
    },
    IfStatement { },
    ImportDirective { },
    IndexAccess { },
    InheritanceSpecifier { },
    InlineAssembly { },
    Literal { },
    Mapping { },
    MemberAccess { },
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
    pub fn identifiers(&self) -> impl Iterator<Item = &str> {
        let mut current = ::std::collections::VecDeque::new();
        current.push_back(self);

        let mut out = Vec::new();

        while let Some(next) = current.pop_front() {
            if let Ast::Identifier { ref attributes, .. } = *next {
                out.push(attributes.value.as_str());
            }

            match *next {
                Ast::FunctionCall { ref children, .. } => {
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
