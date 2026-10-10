pub(crate) mod hourly {
    use sea_orm::entity::prelude::*;
    use sea_orm::sea_query::StringLen;
    #[derive(Clone, Debug, DeriveEntityModel)]
    #[sea_orm(table_name = "request_token_stat_hourly_rollups")]
    pub struct Model {
        #[sea_orm(primary_key, auto_increment = false)]
        pub bucket_start: i64,
        #[sea_orm(
            primary_key,
            auto_increment = false,
            column_type = "String(StringLen::N(128))"
        )]
        pub key_id: String,
        #[sea_orm(
            primary_key,
            auto_increment = false,
            column_type = "String(StringLen::N(128))"
        )]
        pub account_id: String,
        #[sea_orm(
            primary_key,
            auto_increment = false,
            column_type = "String(StringLen::N(191))"
        )]
        pub model: String,
        #[sea_orm(
            primary_key,
            auto_increment = false,
            column_type = "String(StringLen::N(32))"
        )]
        pub actual_source_kind: String,
        #[sea_orm(
            primary_key,
            auto_increment = false,
            column_type = "String(StringLen::N(128))"
        )]
        pub actual_source_id: String,
        #[sea_orm(
            primary_key,
            auto_increment = false,
            column_type = "String(StringLen::N(128))"
        )]
        pub owner_user_id: String,
        pub input_tokens: i64,
        pub cached_input_tokens: i64,
        pub output_tokens: i64,
        pub total_tokens: i64,
        pub reasoning_output_tokens: i64,
        pub estimated_cost_usd: f64,
        pub bucket_end: i64,
        pub request_count: i64,
        pub success_count: i64,
        pub error_count: i64,
        pub updated_at: i64,
    }
    #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
    pub enum Relation {}
    impl ActiveModelBehavior for ActiveModel {}
}

pub(crate) mod legacy {
    use sea_orm::entity::prelude::*;
    use sea_orm::sea_query::StringLen;
    #[derive(Clone, Debug, DeriveEntityModel)]
    #[sea_orm(table_name = "request_token_stat_rollups")]
    pub struct Model {
        #[sea_orm(
            primary_key,
            auto_increment = false,
            column_type = "String(StringLen::N(128))"
        )]
        pub key_id: String,
        #[sea_orm(
            primary_key,
            auto_increment = false,
            column_type = "String(StringLen::N(128))"
        )]
        pub account_id: String,
        #[sea_orm(
            primary_key,
            auto_increment = false,
            column_type = "String(StringLen::N(128))"
        )]
        pub model: String,
        pub input_tokens: i64,
        pub cached_input_tokens: i64,
        pub output_tokens: i64,
        pub total_tokens: i64,
        pub reasoning_output_tokens: i64,
        pub estimated_cost_usd: f64,
        pub source_rows: i64,
        pub updated_at: i64,
    }
    #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
    pub enum Relation {}
    impl ActiveModelBehavior for ActiveModel {}
}
