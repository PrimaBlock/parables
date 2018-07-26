use failure::Error;
use serde::de;
use serde_json;

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
            /// Find the first element exactly matching the given span.
            pub fn find(&self, s: u32, l: u32) -> Option<&Ast> {
                let mut current = ::std::collections::VecDeque::new();
                current.push_back(self);

                while let Some(next) = current.pop_front() {
                    let Src { ref start, ref length, .. } = *next.source();

                    if *start == s && *length == l {
                        return Some(next);
                    }

                    current.extend(next.children());
                }

                None
            }

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

#[derive(Debug)]
pub struct Src {
    start: u32,
    length: u32,
    file_index: u32,
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

#[derive(Debug, Deserialize)]
pub struct VariableDeclarationAttributes {
    name: String,
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
    Identifier { },
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
        attributes: VariableDeclarationAttributes,
    },
    VariableDeclarationStatement { },
}

impl Ast {
    /// Parse AST.
    pub fn parse(input: &str) -> Result<Ast, Error> {
        serde_json::from_str(input).map_err(|e| format_err!("failed to parse AST: {}", e))
    }
}
