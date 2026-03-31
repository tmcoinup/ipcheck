// 拆分说明：原超大文件按职责切分为多个片段，通过 include! 组装，保证行为等价。
include!("mod_prelude.rs");

include!("mod_impl_main.rs");

include!("mod_impl_views.rs");

include!("mod_helpers.rs");
