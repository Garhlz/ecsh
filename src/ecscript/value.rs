#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    Nil,
    Bool(bool),
    Int(i64),
    Float(f64),
    String(String),
    // Array(Rc<RefCell<Vec<Value>>>),
    // Object(Rc<RefCell<HashMap<String, Value>>>),
    // Func(Rc<Func>),
}

impl Value {
    /// 返回可读的类型名，用于错误消息。
    pub fn type_name(&self) -> &'static str {
        match self {
            Value::Nil => "Nil",
            Value::Bool(_) => "Bool",
            Value::Int(_) => "Int",
            Value::Float(_) => "Float",
            Value::String(_) => "String",
        }
    }
}
