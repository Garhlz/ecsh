/// 语句节点。PartialEq 忽略 span（span 是元数据，不影响语义相等）。
#[derive(Debug, Clone)]
pub enum Stmt {
    Let {
        name: String,
        expr: Expr,
        span: usize,
    },
    Assign {
        name: String,
        expr: Expr,
        span: usize,
    },
    ExprStmt {
        expr: Expr,
        span: usize,
    }, // 单个表达式构成的语句
    Block {
        stmts: Vec<Stmt>,
        span: usize,
    }, // 代码块，新的作用域推入栈
}

impl PartialEq for Stmt {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (
                Stmt::Let {
                    name: a, expr: ae, ..
                },
                Stmt::Let {
                    name: b, expr: be, ..
                },
            ) => a == b && ae == be,
            (
                Stmt::Assign {
                    name: a, expr: ae, ..
                },
                Stmt::Assign {
                    name: b, expr: be, ..
                },
            ) => a == b && ae == be,
            (Stmt::ExprStmt { expr: a, .. }, Stmt::ExprStmt { expr: b, .. }) => a == b,
            (Stmt::Block { stmts: a, .. }, Stmt::Block { stmts: b, .. }) => a == b,
            _ => false,
        }
    }
}

impl Stmt {
    pub fn span(&self) -> usize {
        match self {
            Stmt::Let { span, .. } => *span,
            Stmt::Assign { span, .. } => *span,
            Stmt::ExprStmt { span, .. } => *span,
            Stmt::Block { span, .. } => *span,
        }
    }
}

/// 带位置信息的表达式节点。
///
/// span 是源码字节偏移，用于 eval 阶段的错误定位。
/// PartialEq 只比较 kind，忽略 span（span 是元数据，不影响语义相等）。
#[derive(Debug, Clone)]
pub struct Expr {
    pub kind: ExprKind,
    pub span: usize,
}

impl PartialEq for Expr {
    fn eq(&self, other: &Self) -> bool {
        self.kind == other.kind
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum ExprKind {
    Literal(Literal),
    Variable(String), // 只能拿到名字，运行时才知道类型
    Prefix(PrefixOper, Box<Expr>),
    Infix(Box<Expr>, InfixOper, Box<Expr>),
    // 需要显式加Box，避免无限嵌套
    // Assignment{left: Expr, right: Expr},
}

#[derive(Debug, Clone, PartialEq)]
pub enum Literal {
    Nil,
    Bool(bool),
    Int(i64),
    Float(f64),
    String(String),
}

#[derive(Debug, Clone, PartialEq)]
pub enum PrefixOper {
    Neg, // 负值
    Not, // 逻辑非
}

#[derive(Debug, Clone, PartialEq)]
pub enum InfixOper {
    // 算术运算符
    Add, // +
    Sub, // -
    Mul, // *
    Div, // /
    Mod, // %

    // 比较运算符
    Eq, // ==
    Ne, // !=
    Lt, // <
    Gt, // >
    Le, // <=
    Ge, // >=

    // 逻辑运算符
    And, // &&
    Or,  // ||
}
