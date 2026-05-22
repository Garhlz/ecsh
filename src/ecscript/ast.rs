/// 语句节点。PartialEq 忽略 span（span 是元数据，不影响语义相等）。
#[derive(Debug, Clone)]
pub enum Stmt {
    Let {
        name: String,
        expr: Expr,
        span: usize,
    },
    Assign {
        target: AssignTarget,
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
    If {
        cond: Expr,
        then_body: Vec<Stmt>,
        else_body: Vec<Stmt>,
        span: usize,
    },
    While {
        cond: Expr,
        body: Vec<Stmt>,
        span: usize,
    },
    ForIn {
        var: String,
        iterable: Expr,
        body: Vec<Stmt>,
        span: usize,
    },
    ForRange {
        var: String,
        range: RangeExpr,
        body: Vec<Stmt>,
        span: usize,
    },
    // break continue也是完整的语句
    Break {
        span: usize,
    },
    Continue {
        span: usize,
    },
}
#[derive(Debug, Clone, PartialEq)]
pub struct RangeExpr {
    pub start: Box<Expr>,
    pub end: Box<Expr>,
    pub inclusive: bool, // 1..10 = false, 1..=10 = true
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
                    target: a,
                    expr: ae,
                    ..
                },
                Stmt::Assign {
                    target: b,
                    expr: be,
                    ..
                },
            ) => a == b && ae == be,
            (Stmt::ExprStmt { expr: a, .. }, Stmt::ExprStmt { expr: b, .. }) => a == b,
            (Stmt::Block { stmts: a, .. }, Stmt::Block { stmts: b, .. }) => a == b,
            (
                Stmt::If {
                    cond: ac,
                    then_body: at,
                    else_body: ae,
                    ..
                },
                Stmt::If {
                    cond: bc,
                    then_body: bt,
                    else_body: be,
                    ..
                },
            ) => ac == bc && at == bt && ae == be,
            (
                Stmt::While {
                    cond: ac, body: ab, ..
                },
                Stmt::While {
                    cond: bc, body: bb, ..
                },
            ) => ac == bc && ab == bb,
            (
                Stmt::ForIn {
                    var: av,
                    iterable: ai,
                    body: ab,
                    ..
                },
                Stmt::ForIn {
                    var: bv,
                    iterable: bi,
                    body: bb,
                    ..
                },
            ) => av == bv && ai == bi && ab == bb,
            (
                Stmt::ForRange {
                    var: av,
                    range: ar,
                    body: ab,
                    ..
                },
                Stmt::ForRange {
                    var: bv,
                    range: br,
                    body: bb,
                    ..
                },
            ) => av == bv && ar == br && ab == bb,
            (Stmt::Break { .. }, Stmt::Break { .. }) => true,
            (Stmt::Continue { .. }, Stmt::Continue { .. }) => true,
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
            // 控制流
            Stmt::If { span, .. } => *span,
            Stmt::While { span, .. } => *span,
            Stmt::ForIn { span, .. } => *span,
            Stmt::ForRange { span, .. } => *span,
            Stmt::Break { span } => *span,
            Stmt::Continue { span } => *span,
        }
    }
}
#[derive(Debug, Clone)]
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

impl PartialEq for AssignTarget {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (AssignTarget::Name(a), AssignTarget::Name(b)) => a == b,
            (
                AssignTarget::Field {
                    object: e1,
                    field: f1,
                },
                AssignTarget::Field {
                    object: e2,
                    field: f2,
                },
            ) => e1 == e2 && f1 == f2,
            (
                AssignTarget::Index {
                    object: a1,
                    index: i1,
                },
                AssignTarget::Index {
                    object: a2,
                    index: i2,
                },
            ) => a1 == a2 && i1 == i2,
            _ => false,
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
    Literal(Literal), // 字面量
    Variable(String), // 变量，在ast阶段只有名称
    Prefix(PrefixOper, Box<Expr>),
    Infix(Box<Expr>, InfixOper, Box<Expr>), // 需要显式加Box，避免无限嵌套

    // 数组和对象
    Array(Vec<Expr>),            // 数组的字面量表达式 eg:[1,2,3]
    Object(Vec<(String, Expr)>), // 对象的字面量表达式 eg: {"a": 1, "b": 2}
    // Object 的key是不是改成Expr比较好，如果是表达式的话，其值只有eval期间才可以得到
    Index(Box<Expr>, Box<Expr>), // 数组随机访问的表达式 eg: arr[0]
    Field(Box<Expr>, String),    // 对象字段访问的表达式 eg: obj.name

    // 函数调用
    Call(Box<Expr>, Vec<Expr>),

    // 循环遍历的 1..10 / 1..=10，也视为表达式。但是暂时只能出现于for i in 1..10语法中
    Range(RangeExpr),
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
