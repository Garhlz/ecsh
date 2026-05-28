use super::value::CommandValue;
/// 语句内容（不含位置信息），可直接 derive PartialEq。
#[derive(Debug, Clone, PartialEq)]
pub enum StmtKind {
    Let {
        name: String,
        expr: Expr,
        public: bool,
    },
    Assign {
        target: AssignTarget,
        expr: Expr,
    },
    CompoundAssign {
        target: AssignTarget,
        op: CompoundAssignOp,
        expr: Expr,
    },
    ExprStmt {
        expr: Expr,
    }, // 单个表达式构成的语句
    Block {
        stmts: Vec<Stmt>,
    }, // 代码块，新的作用域推入栈
    If {
        cond: Expr,
        then_body: Vec<Stmt>,
        else_body: Vec<Stmt>,
    },
    While {
        cond: Expr,
        body: Vec<Stmt>,
    },
    ForIn {
        var: String,
        iterable: Expr,
        body: Vec<Stmt>,
    },
    ForRange {
        var: String,
        range: RangeExpr,
        body: Vec<Stmt>,
    },
    FuncDeclare {
        name: String,
        params: Vec<String>,
        body: Vec<Stmt>,
        public: bool,
    },
    Use {
        path: String,
        alias: String,
    },

    // break continue也是完整的语句
    Break,
    Continue,
    Return {
        value: Option<Expr>,
    },
}

/// 带位置信息的语句节点。
///
/// span 是源码字节偏移，用于 eval 阶段的错误定位。
/// PartialEq 只比较 kind，忽略 span（span 是元数据，不影响语义相等）。
#[derive(Debug, Clone)]
pub struct Stmt {
    pub kind: StmtKind,
    pub span: usize,
}

impl PartialEq for Stmt {
    fn eq(&self, other: &Self) -> bool {
        self.kind == other.kind
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompoundAssignOp {
    Add,
    Sub,
    Mul,
    Div,
    Mod,
}

#[derive(Debug, Clone, PartialEq)]
pub struct RangeExpr {
    pub start: Box<Expr>,
    pub end: Box<Expr>,
    pub inclusive: bool, // 1..10 = false, 1..=10 = true
}

#[derive(Debug, Clone, PartialEq)]
pub enum AssignTarget {
    Name(String),
    Field { object: Expr, field: String }, // obj.name
    Index { object: Expr, index: Expr },   // arr[i]
}

pub fn expr_to_assign_target(expr: &Expr) -> Option<AssignTarget> {
    match expr.kind.clone() {
        ExprKind::Variable(name) => Some(AssignTarget::Name(name)),
        ExprKind::Field(obj, field) => Some(AssignTarget::Field {
            object: *obj,
            field: field,
        }),

        ExprKind::Index(object, index) => Some(AssignTarget::Index {
            object: *object,
            index: *index,
        }),

        _ => None,
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
    Literal(Literal), // 字面量
    Variable(String), // 变量，在ast阶段只有名称
    Prefix(PrefixOper, Box<Expr>),
    Infix(Box<Expr>, InfixOper, Box<Expr>), // 需要显式加Box，避免无限嵌套

    // 数组和对象
    Array(Vec<Expr>),            // 数组的字面量表达式 eg:[1,2,3]
    Object(Vec<(String, Expr)>), // 对象的字面量表达式 eg: {"a": 1, "b": 2}
    Index(Box<Expr>, Box<Expr>), // 数组随机访问的表达式 eg: arr[0]
    Field(Box<Expr>, String),    // 对象字段访问的表达式 eg: obj.name

    // 函数调用
    Call(Box<Expr>, Vec<Expr>),

    // 循环遍历的 1..10 / 1..=10，也视为表达式。但是暂时只能出现于for i in 1..10语法中
    Range(RangeExpr),

    // 匿名函数（或lambda表达式）
    FuncLiteral {
        params: Vec<String>,
        body: Vec<Stmt>,
    },

    CommandLiteral(CommandValue),
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

    PipeForward,
}
