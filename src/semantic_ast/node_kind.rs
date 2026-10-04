//! Generated from the JDT DOM (`ASTNode.nodeClassForType`): node types,
//! their JDT node type constants and class hierarchy.

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum NodeKind {
    AnonymousClassDeclaration = 1,
    ArrayAccess = 2,
    ArrayCreation = 3,
    ArrayInitializer = 4,
    ArrayType = 5,
    AssertStatement = 6,
    Assignment = 7,
    Block = 8,
    BooleanLiteral = 9,
    BreakStatement = 10,
    CastExpression = 11,
    CatchClause = 12,
    CharacterLiteral = 13,
    ClassInstanceCreation = 14,
    CompilationUnit = 15,
    ConditionalExpression = 16,
    ConstructorInvocation = 17,
    ContinueStatement = 18,
    DoStatement = 19,
    EmptyStatement = 20,
    ExpressionStatement = 21,
    FieldAccess = 22,
    FieldDeclaration = 23,
    ForStatement = 24,
    IfStatement = 25,
    ImportDeclaration = 26,
    InfixExpression = 27,
    Initializer = 28,
    Javadoc = 29,
    LabeledStatement = 30,
    MethodDeclaration = 31,
    MethodInvocation = 32,
    NullLiteral = 33,
    NumberLiteral = 34,
    PackageDeclaration = 35,
    ParenthesizedExpression = 36,
    PostfixExpression = 37,
    PrefixExpression = 38,
    PrimitiveType = 39,
    QualifiedName = 40,
    ReturnStatement = 41,
    SimpleName = 42,
    SimpleType = 43,
    SingleVariableDeclaration = 44,
    StringLiteral = 45,
    SuperConstructorInvocation = 46,
    SuperFieldAccess = 47,
    SuperMethodInvocation = 48,
    SwitchCase = 49,
    SwitchStatement = 50,
    SynchronizedStatement = 51,
    ThisExpression = 52,
    ThrowStatement = 53,
    TryStatement = 54,
    TypeDeclaration = 55,
    TypeDeclarationStatement = 56,
    TypeLiteral = 57,
    VariableDeclarationExpression = 58,
    VariableDeclarationFragment = 59,
    VariableDeclarationStatement = 60,
    WhileStatement = 61,
    InstanceofExpression = 62,
    LineComment = 63,
    BlockComment = 64,
    TagElement = 65,
    TextElement = 66,
    MemberRef = 67,
    MethodRef = 68,
    MethodRefParameter = 69,
    EnhancedForStatement = 70,
    EnumDeclaration = 71,
    EnumConstantDeclaration = 72,
    TypeParameter = 73,
    ParameterizedType = 74,
    QualifiedType = 75,
    WildcardType = 76,
    NormalAnnotation = 77,
    MarkerAnnotation = 78,
    SingleMemberAnnotation = 79,
    MemberValuePair = 80,
    AnnotationTypeDeclaration = 81,
    AnnotationTypeMemberDeclaration = 82,
    Modifier = 83,
    UnionType = 84,
    Dimension = 85,
    LambdaExpression = 86,
    IntersectionType = 87,
    NameQualifiedType = 88,
    CreationReference = 89,
    ExpressionMethodReference = 90,
    SuperMethodReference = 91,
    TypeMethodReference = 92,
    ModuleDeclaration = 93,
    RequiresDirective = 94,
    ExportsDirective = 95,
    OpensDirective = 96,
    UsesDirective = 97,
    ProvidesDirective = 98,
    ModuleModifier = 99,
    SwitchExpression = 100,
    YieldStatement = 101,
    TextBlock = 102,
    RecordDeclaration = 103,
    PatternInstanceofExpression = 104,
    ModuleQualifiedName = 105,
    TypePattern = 106,
    GuardedPattern = 107,
    NullPattern = 108,
    CaseDefaultExpression = 109,
    TagProperty = 110,
    JavaDocRegion = 111,
    JavaDocTextElement = 112,
    RecordPattern = 113,
    EitherOrMultiPattern = 114,
    ImplicitTypeDeclaration = 115,
    /// A node type this model does not know.
    Unknown = 0,
}

impl NodeKind {
    pub fn from_name(name: &str) -> NodeKind {
        match name {
            "AnonymousClassDeclaration" => NodeKind::AnonymousClassDeclaration,
            "ArrayAccess" => NodeKind::ArrayAccess,
            "ArrayCreation" => NodeKind::ArrayCreation,
            "ArrayInitializer" => NodeKind::ArrayInitializer,
            "ArrayType" => NodeKind::ArrayType,
            "AssertStatement" => NodeKind::AssertStatement,
            "Assignment" => NodeKind::Assignment,
            "Block" => NodeKind::Block,
            "BooleanLiteral" => NodeKind::BooleanLiteral,
            "BreakStatement" => NodeKind::BreakStatement,
            "CastExpression" => NodeKind::CastExpression,
            "CatchClause" => NodeKind::CatchClause,
            "CharacterLiteral" => NodeKind::CharacterLiteral,
            "ClassInstanceCreation" => NodeKind::ClassInstanceCreation,
            "CompilationUnit" => NodeKind::CompilationUnit,
            "ConditionalExpression" => NodeKind::ConditionalExpression,
            "ConstructorInvocation" => NodeKind::ConstructorInvocation,
            "ContinueStatement" => NodeKind::ContinueStatement,
            "DoStatement" => NodeKind::DoStatement,
            "EmptyStatement" => NodeKind::EmptyStatement,
            "ExpressionStatement" => NodeKind::ExpressionStatement,
            "FieldAccess" => NodeKind::FieldAccess,
            "FieldDeclaration" => NodeKind::FieldDeclaration,
            "ForStatement" => NodeKind::ForStatement,
            "IfStatement" => NodeKind::IfStatement,
            "ImportDeclaration" => NodeKind::ImportDeclaration,
            "InfixExpression" => NodeKind::InfixExpression,
            "Initializer" => NodeKind::Initializer,
            "Javadoc" => NodeKind::Javadoc,
            "LabeledStatement" => NodeKind::LabeledStatement,
            "MethodDeclaration" => NodeKind::MethodDeclaration,
            "MethodInvocation" => NodeKind::MethodInvocation,
            "NullLiteral" => NodeKind::NullLiteral,
            "NumberLiteral" => NodeKind::NumberLiteral,
            "PackageDeclaration" => NodeKind::PackageDeclaration,
            "ParenthesizedExpression" => NodeKind::ParenthesizedExpression,
            "PostfixExpression" => NodeKind::PostfixExpression,
            "PrefixExpression" => NodeKind::PrefixExpression,
            "PrimitiveType" => NodeKind::PrimitiveType,
            "QualifiedName" => NodeKind::QualifiedName,
            "ReturnStatement" => NodeKind::ReturnStatement,
            "SimpleName" => NodeKind::SimpleName,
            "SimpleType" => NodeKind::SimpleType,
            "SingleVariableDeclaration" => NodeKind::SingleVariableDeclaration,
            "StringLiteral" => NodeKind::StringLiteral,
            "SuperConstructorInvocation" => NodeKind::SuperConstructorInvocation,
            "SuperFieldAccess" => NodeKind::SuperFieldAccess,
            "SuperMethodInvocation" => NodeKind::SuperMethodInvocation,
            "SwitchCase" => NodeKind::SwitchCase,
            "SwitchStatement" => NodeKind::SwitchStatement,
            "SynchronizedStatement" => NodeKind::SynchronizedStatement,
            "ThisExpression" => NodeKind::ThisExpression,
            "ThrowStatement" => NodeKind::ThrowStatement,
            "TryStatement" => NodeKind::TryStatement,
            "TypeDeclaration" => NodeKind::TypeDeclaration,
            "TypeDeclarationStatement" => NodeKind::TypeDeclarationStatement,
            "TypeLiteral" => NodeKind::TypeLiteral,
            "VariableDeclarationExpression" => NodeKind::VariableDeclarationExpression,
            "VariableDeclarationFragment" => NodeKind::VariableDeclarationFragment,
            "VariableDeclarationStatement" => NodeKind::VariableDeclarationStatement,
            "WhileStatement" => NodeKind::WhileStatement,
            "InstanceofExpression" => NodeKind::InstanceofExpression,
            "LineComment" => NodeKind::LineComment,
            "BlockComment" => NodeKind::BlockComment,
            "TagElement" => NodeKind::TagElement,
            "TextElement" => NodeKind::TextElement,
            "MemberRef" => NodeKind::MemberRef,
            "MethodRef" => NodeKind::MethodRef,
            "MethodRefParameter" => NodeKind::MethodRefParameter,
            "EnhancedForStatement" => NodeKind::EnhancedForStatement,
            "EnumDeclaration" => NodeKind::EnumDeclaration,
            "EnumConstantDeclaration" => NodeKind::EnumConstantDeclaration,
            "TypeParameter" => NodeKind::TypeParameter,
            "ParameterizedType" => NodeKind::ParameterizedType,
            "QualifiedType" => NodeKind::QualifiedType,
            "WildcardType" => NodeKind::WildcardType,
            "NormalAnnotation" => NodeKind::NormalAnnotation,
            "MarkerAnnotation" => NodeKind::MarkerAnnotation,
            "SingleMemberAnnotation" => NodeKind::SingleMemberAnnotation,
            "MemberValuePair" => NodeKind::MemberValuePair,
            "AnnotationTypeDeclaration" => NodeKind::AnnotationTypeDeclaration,
            "AnnotationTypeMemberDeclaration" => NodeKind::AnnotationTypeMemberDeclaration,
            "Modifier" => NodeKind::Modifier,
            "UnionType" => NodeKind::UnionType,
            "Dimension" => NodeKind::Dimension,
            "LambdaExpression" => NodeKind::LambdaExpression,
            "IntersectionType" => NodeKind::IntersectionType,
            "NameQualifiedType" => NodeKind::NameQualifiedType,
            "CreationReference" => NodeKind::CreationReference,
            "ExpressionMethodReference" => NodeKind::ExpressionMethodReference,
            "SuperMethodReference" => NodeKind::SuperMethodReference,
            "TypeMethodReference" => NodeKind::TypeMethodReference,
            "ModuleDeclaration" => NodeKind::ModuleDeclaration,
            "RequiresDirective" => NodeKind::RequiresDirective,
            "ExportsDirective" => NodeKind::ExportsDirective,
            "OpensDirective" => NodeKind::OpensDirective,
            "UsesDirective" => NodeKind::UsesDirective,
            "ProvidesDirective" => NodeKind::ProvidesDirective,
            "ModuleModifier" => NodeKind::ModuleModifier,
            "SwitchExpression" => NodeKind::SwitchExpression,
            "YieldStatement" => NodeKind::YieldStatement,
            "TextBlock" => NodeKind::TextBlock,
            "RecordDeclaration" => NodeKind::RecordDeclaration,
            "PatternInstanceofExpression" => NodeKind::PatternInstanceofExpression,
            "ModuleQualifiedName" => NodeKind::ModuleQualifiedName,
            "TypePattern" => NodeKind::TypePattern,
            "GuardedPattern" => NodeKind::GuardedPattern,
            "NullPattern" => NodeKind::NullPattern,
            "CaseDefaultExpression" => NodeKind::CaseDefaultExpression,
            "TagProperty" => NodeKind::TagProperty,
            "JavaDocRegion" => NodeKind::JavaDocRegion,
            "JavaDocTextElement" => NodeKind::JavaDocTextElement,
            "RecordPattern" => NodeKind::RecordPattern,
            "EitherOrMultiPattern" => NodeKind::EitherOrMultiPattern,
            "ImplicitTypeDeclaration" => NodeKind::ImplicitTypeDeclaration,
            _ => NodeKind::Unknown,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            NodeKind::AnonymousClassDeclaration => "AnonymousClassDeclaration",
            NodeKind::ArrayAccess => "ArrayAccess",
            NodeKind::ArrayCreation => "ArrayCreation",
            NodeKind::ArrayInitializer => "ArrayInitializer",
            NodeKind::ArrayType => "ArrayType",
            NodeKind::AssertStatement => "AssertStatement",
            NodeKind::Assignment => "Assignment",
            NodeKind::Block => "Block",
            NodeKind::BooleanLiteral => "BooleanLiteral",
            NodeKind::BreakStatement => "BreakStatement",
            NodeKind::CastExpression => "CastExpression",
            NodeKind::CatchClause => "CatchClause",
            NodeKind::CharacterLiteral => "CharacterLiteral",
            NodeKind::ClassInstanceCreation => "ClassInstanceCreation",
            NodeKind::CompilationUnit => "CompilationUnit",
            NodeKind::ConditionalExpression => "ConditionalExpression",
            NodeKind::ConstructorInvocation => "ConstructorInvocation",
            NodeKind::ContinueStatement => "ContinueStatement",
            NodeKind::DoStatement => "DoStatement",
            NodeKind::EmptyStatement => "EmptyStatement",
            NodeKind::ExpressionStatement => "ExpressionStatement",
            NodeKind::FieldAccess => "FieldAccess",
            NodeKind::FieldDeclaration => "FieldDeclaration",
            NodeKind::ForStatement => "ForStatement",
            NodeKind::IfStatement => "IfStatement",
            NodeKind::ImportDeclaration => "ImportDeclaration",
            NodeKind::InfixExpression => "InfixExpression",
            NodeKind::Initializer => "Initializer",
            NodeKind::Javadoc => "Javadoc",
            NodeKind::LabeledStatement => "LabeledStatement",
            NodeKind::MethodDeclaration => "MethodDeclaration",
            NodeKind::MethodInvocation => "MethodInvocation",
            NodeKind::NullLiteral => "NullLiteral",
            NodeKind::NumberLiteral => "NumberLiteral",
            NodeKind::PackageDeclaration => "PackageDeclaration",
            NodeKind::ParenthesizedExpression => "ParenthesizedExpression",
            NodeKind::PostfixExpression => "PostfixExpression",
            NodeKind::PrefixExpression => "PrefixExpression",
            NodeKind::PrimitiveType => "PrimitiveType",
            NodeKind::QualifiedName => "QualifiedName",
            NodeKind::ReturnStatement => "ReturnStatement",
            NodeKind::SimpleName => "SimpleName",
            NodeKind::SimpleType => "SimpleType",
            NodeKind::SingleVariableDeclaration => "SingleVariableDeclaration",
            NodeKind::StringLiteral => "StringLiteral",
            NodeKind::SuperConstructorInvocation => "SuperConstructorInvocation",
            NodeKind::SuperFieldAccess => "SuperFieldAccess",
            NodeKind::SuperMethodInvocation => "SuperMethodInvocation",
            NodeKind::SwitchCase => "SwitchCase",
            NodeKind::SwitchStatement => "SwitchStatement",
            NodeKind::SynchronizedStatement => "SynchronizedStatement",
            NodeKind::ThisExpression => "ThisExpression",
            NodeKind::ThrowStatement => "ThrowStatement",
            NodeKind::TryStatement => "TryStatement",
            NodeKind::TypeDeclaration => "TypeDeclaration",
            NodeKind::TypeDeclarationStatement => "TypeDeclarationStatement",
            NodeKind::TypeLiteral => "TypeLiteral",
            NodeKind::VariableDeclarationExpression => "VariableDeclarationExpression",
            NodeKind::VariableDeclarationFragment => "VariableDeclarationFragment",
            NodeKind::VariableDeclarationStatement => "VariableDeclarationStatement",
            NodeKind::WhileStatement => "WhileStatement",
            NodeKind::InstanceofExpression => "InstanceofExpression",
            NodeKind::LineComment => "LineComment",
            NodeKind::BlockComment => "BlockComment",
            NodeKind::TagElement => "TagElement",
            NodeKind::TextElement => "TextElement",
            NodeKind::MemberRef => "MemberRef",
            NodeKind::MethodRef => "MethodRef",
            NodeKind::MethodRefParameter => "MethodRefParameter",
            NodeKind::EnhancedForStatement => "EnhancedForStatement",
            NodeKind::EnumDeclaration => "EnumDeclaration",
            NodeKind::EnumConstantDeclaration => "EnumConstantDeclaration",
            NodeKind::TypeParameter => "TypeParameter",
            NodeKind::ParameterizedType => "ParameterizedType",
            NodeKind::QualifiedType => "QualifiedType",
            NodeKind::WildcardType => "WildcardType",
            NodeKind::NormalAnnotation => "NormalAnnotation",
            NodeKind::MarkerAnnotation => "MarkerAnnotation",
            NodeKind::SingleMemberAnnotation => "SingleMemberAnnotation",
            NodeKind::MemberValuePair => "MemberValuePair",
            NodeKind::AnnotationTypeDeclaration => "AnnotationTypeDeclaration",
            NodeKind::AnnotationTypeMemberDeclaration => "AnnotationTypeMemberDeclaration",
            NodeKind::Modifier => "Modifier",
            NodeKind::UnionType => "UnionType",
            NodeKind::Dimension => "Dimension",
            NodeKind::LambdaExpression => "LambdaExpression",
            NodeKind::IntersectionType => "IntersectionType",
            NodeKind::NameQualifiedType => "NameQualifiedType",
            NodeKind::CreationReference => "CreationReference",
            NodeKind::ExpressionMethodReference => "ExpressionMethodReference",
            NodeKind::SuperMethodReference => "SuperMethodReference",
            NodeKind::TypeMethodReference => "TypeMethodReference",
            NodeKind::ModuleDeclaration => "ModuleDeclaration",
            NodeKind::RequiresDirective => "RequiresDirective",
            NodeKind::ExportsDirective => "ExportsDirective",
            NodeKind::OpensDirective => "OpensDirective",
            NodeKind::UsesDirective => "UsesDirective",
            NodeKind::ProvidesDirective => "ProvidesDirective",
            NodeKind::ModuleModifier => "ModuleModifier",
            NodeKind::SwitchExpression => "SwitchExpression",
            NodeKind::YieldStatement => "YieldStatement",
            NodeKind::TextBlock => "TextBlock",
            NodeKind::RecordDeclaration => "RecordDeclaration",
            NodeKind::PatternInstanceofExpression => "PatternInstanceofExpression",
            NodeKind::ModuleQualifiedName => "ModuleQualifiedName",
            NodeKind::TypePattern => "TypePattern",
            NodeKind::GuardedPattern => "GuardedPattern",
            NodeKind::NullPattern => "NullPattern",
            NodeKind::CaseDefaultExpression => "CaseDefaultExpression",
            NodeKind::TagProperty => "TagProperty",
            NodeKind::JavaDocRegion => "JavaDocRegion",
            NodeKind::JavaDocTextElement => "JavaDocTextElement",
            NodeKind::RecordPattern => "RecordPattern",
            NodeKind::EitherOrMultiPattern => "EitherOrMultiPattern",
            NodeKind::ImplicitTypeDeclaration => "ImplicitTypeDeclaration",
            NodeKind::Unknown => "Unknown",
        }
    }

    /// `ASTNode.getNodeType()`.
    pub fn node_type(self) -> u32 {
        self as u32
    }

    /// `instanceof AbstractTagElement`.
    pub fn is_abstract_tag_element(self) -> bool {
        matches!(self, NodeKind::TagElement | NodeKind::JavaDocRegion)
    }

    /// `instanceof AbstractTextElement`.
    pub fn is_abstract_text_element(self) -> bool {
        matches!(self, NodeKind::TextElement | NodeKind::JavaDocTextElement)
    }

    /// `instanceof AbstractTypeDeclaration`.
    pub fn is_abstract_type_declaration(self) -> bool {
        matches!(self, NodeKind::TypeDeclaration | NodeKind::EnumDeclaration | NodeKind::AnnotationTypeDeclaration | NodeKind::RecordDeclaration | NodeKind::ImplicitTypeDeclaration)
    }

    /// `instanceof AnnotatableType`.
    pub fn is_annotatable_type(self) -> bool {
        matches!(self, NodeKind::PrimitiveType | NodeKind::SimpleType | NodeKind::QualifiedType | NodeKind::WildcardType | NodeKind::NameQualifiedType)
    }

    /// `instanceof Annotation`.
    pub fn is_annotation(self) -> bool {
        matches!(self, NodeKind::NormalAnnotation | NodeKind::MarkerAnnotation | NodeKind::SingleMemberAnnotation)
    }

    /// `instanceof BodyDeclaration`.
    pub fn is_body_declaration(self) -> bool {
        matches!(self, NodeKind::FieldDeclaration | NodeKind::Initializer | NodeKind::MethodDeclaration | NodeKind::TypeDeclaration | NodeKind::EnumDeclaration | NodeKind::EnumConstantDeclaration | NodeKind::AnnotationTypeDeclaration | NodeKind::AnnotationTypeMemberDeclaration | NodeKind::RecordDeclaration | NodeKind::ImplicitTypeDeclaration)
    }

    /// `instanceof Comment`.
    pub fn is_comment(self) -> bool {
        matches!(self, NodeKind::Javadoc | NodeKind::LineComment | NodeKind::BlockComment)
    }

    /// `instanceof Expression`.
    pub fn is_expression(self) -> bool {
        matches!(self, NodeKind::ArrayAccess | NodeKind::ArrayCreation | NodeKind::ArrayInitializer | NodeKind::Assignment | NodeKind::BooleanLiteral | NodeKind::CastExpression | NodeKind::CharacterLiteral | NodeKind::ClassInstanceCreation | NodeKind::ConditionalExpression | NodeKind::FieldAccess | NodeKind::InfixExpression | NodeKind::MethodInvocation | NodeKind::NullLiteral | NodeKind::NumberLiteral | NodeKind::ParenthesizedExpression | NodeKind::PostfixExpression | NodeKind::PrefixExpression | NodeKind::QualifiedName | NodeKind::SimpleName | NodeKind::StringLiteral | NodeKind::SuperFieldAccess | NodeKind::SuperMethodInvocation | NodeKind::ThisExpression | NodeKind::TypeLiteral | NodeKind::VariableDeclarationExpression | NodeKind::InstanceofExpression | NodeKind::NormalAnnotation | NodeKind::MarkerAnnotation | NodeKind::SingleMemberAnnotation | NodeKind::LambdaExpression | NodeKind::CreationReference | NodeKind::ExpressionMethodReference | NodeKind::SuperMethodReference | NodeKind::TypeMethodReference | NodeKind::SwitchExpression | NodeKind::TextBlock | NodeKind::PatternInstanceofExpression | NodeKind::ModuleQualifiedName | NodeKind::TypePattern | NodeKind::GuardedPattern | NodeKind::NullPattern | NodeKind::CaseDefaultExpression | NodeKind::RecordPattern | NodeKind::EitherOrMultiPattern)
    }

    /// `instanceof MethodReference`.
    pub fn is_method_reference(self) -> bool {
        matches!(self, NodeKind::CreationReference | NodeKind::ExpressionMethodReference | NodeKind::SuperMethodReference | NodeKind::TypeMethodReference)
    }

    /// `instanceof ModuleDirective`.
    pub fn is_module_directive(self) -> bool {
        matches!(self, NodeKind::RequiresDirective | NodeKind::ExportsDirective | NodeKind::OpensDirective | NodeKind::UsesDirective | NodeKind::ProvidesDirective)
    }

    /// `instanceof ModulePackageAccess`.
    pub fn is_module_package_access(self) -> bool {
        matches!(self, NodeKind::ExportsDirective | NodeKind::OpensDirective)
    }

    /// `instanceof Name`.
    pub fn is_name(self) -> bool {
        matches!(self, NodeKind::QualifiedName | NodeKind::SimpleName | NodeKind::ModuleQualifiedName)
    }

    /// `instanceof Pattern`.
    pub fn is_pattern(self) -> bool {
        matches!(self, NodeKind::TypePattern | NodeKind::GuardedPattern | NodeKind::NullPattern | NodeKind::RecordPattern | NodeKind::EitherOrMultiPattern)
    }

    /// `instanceof Statement`.
    pub fn is_statement(self) -> bool {
        matches!(self, NodeKind::AssertStatement | NodeKind::Block | NodeKind::BreakStatement | NodeKind::ConstructorInvocation | NodeKind::ContinueStatement | NodeKind::DoStatement | NodeKind::EmptyStatement | NodeKind::ExpressionStatement | NodeKind::ForStatement | NodeKind::IfStatement | NodeKind::LabeledStatement | NodeKind::ReturnStatement | NodeKind::SuperConstructorInvocation | NodeKind::SwitchCase | NodeKind::SwitchStatement | NodeKind::SynchronizedStatement | NodeKind::ThrowStatement | NodeKind::TryStatement | NodeKind::TypeDeclarationStatement | NodeKind::VariableDeclarationStatement | NodeKind::WhileStatement | NodeKind::EnhancedForStatement | NodeKind::YieldStatement)
    }

    /// `instanceof Type`.
    pub fn is_type(self) -> bool {
        matches!(self, NodeKind::ArrayType | NodeKind::PrimitiveType | NodeKind::SimpleType | NodeKind::ParameterizedType | NodeKind::QualifiedType | NodeKind::WildcardType | NodeKind::UnionType | NodeKind::IntersectionType | NodeKind::NameQualifiedType)
    }

    /// `instanceof VariableDeclaration`.
    pub fn is_variable_declaration(self) -> bool {
        matches!(self, NodeKind::SingleVariableDeclaration | NodeKind::VariableDeclarationFragment)
    }

}
