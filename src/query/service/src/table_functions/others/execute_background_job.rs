// Copyright 2021 Datafuse Labs
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

use std::any::Any;
use std::sync::Arc;

use background_service::get_background_service_handler;
use chrono::NaiveDateTime;
use chrono::TimeZone;
use chrono::Utc;
use common_catalog::plan::DataSourcePlan;
use common_catalog::plan::PartStatistics;
use common_catalog::plan::Partitions;
use common_catalog::plan::PushDownInfo;
use common_catalog::table_args::TableArgs;
use common_catalog::table_context::TableContext;
use common_catalog::table_function::TableFunction;
pub use common_exception::Result;
use common_expression::DataBlock;
use common_expression::Scalar;
use common_expression::TableSchema;
use common_meta_app::schema::TableIdent;
use common_meta_app::schema::TableInfo;
use common_meta_app::schema::TableMeta;
use common_pipeline_core::processors::port::OutputPort;
use common_pipeline_core::processors::processor::ProcessorPtr;
use common_pipeline_core::Pipeline;
use common_pipeline_sources::AsyncSource;
use common_pipeline_sources::AsyncSourcer;
use common_storages_factory::Table;

pub struct ExecuteBackgroundJobTable {
    job_name: String,
    table_info: TableInfo,
}

impl ExecuteBackgroundJobTable {
    pub fn create(
        database_name: &str,
        table_func_name: &str,
        table_id: u64,
        table_args: TableArgs,
    ) -> Result<Arc<dyn TableFunction>> {
        let args = table_args.expect_all_positioned(table_func_name, Some(1))?;
        let job_name = String::from_utf8_lossy(args[0].as_string().unwrap().as_slice()).to_string();

        let table_info = TableInfo {
            ident: TableIdent::new(table_id, 0),
            desc: format!("'{}'.'{}'", database_name, table_func_name),
            name: String::from("execute_background_job"),
            meta: TableMeta {
                schema: Arc::new(TableSchema::empty()),
                engine: String::from(table_func_name),
                // Assuming that created_on is unnecessary for function table,
                // we could make created_on fixed to pass test_shuffle_action_try_into.
                created_on: Utc
                    .from_utc_datetime(&NaiveDateTime::from_timestamp_opt(0, 0).unwrap()),
                updated_on: Utc
                    .from_utc_datetime(&NaiveDateTime::from_timestamp_opt(0, 0).unwrap()),
                ..Default::default()
            },
            ..Default::default()
        };

        Ok(Arc::new(ExecuteBackgroundJobTable {
            table_info,
            job_name,
        }))
    }
}

#[async_trait::async_trait]
impl Table for ExecuteBackgroundJobTable {
    fn is_local(&self) -> bool {
        true
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn get_table_info(&self) -> &TableInfo {
        &self.table_info
    }

    #[async_backtrace::framed]
    async fn read_partitions(
        &self,
        _ctx: Arc<dyn TableContext>,
        _push_downs: Option<PushDownInfo>,
        _dry_run: bool,
    ) -> Result<(PartStatistics, Partitions)> {
        // dummy statistics
        Ok((PartStatistics::new_exact(1, 1, 1, 1), Partitions::default()))
    }

    fn table_args(&self) -> Option<TableArgs> {
        Some(TableArgs::new_positioned(vec![Scalar::String(
            self.job_name.as_bytes().to_vec(),
        )]))
    }

    fn read_data(
        &self,
        ctx: Arc<dyn TableContext>,
        _plan: &DataSourcePlan,
        pipeline: &mut Pipeline,
    ) -> Result<()> {
        pipeline.add_source(
            |output| ExecuteBackgroundJobSource::create(ctx.clone(), output, self.job_name.clone()),
            1,
        )?;

        Ok(())
    }
}

struct ExecuteBackgroundJobSource {
    job_name: String,
    ctx: Arc<dyn TableContext>,
}

impl ExecuteBackgroundJobSource {
    pub fn create(
        ctx: Arc<dyn TableContext>,
        output: Arc<OutputPort>,
        job_name: String,
    ) -> Result<ProcessorPtr> {
        AsyncSourcer::create(ctx.clone(), output, ExecuteBackgroundJobSource {
            ctx,
            job_name,
        })
    }
}

#[async_trait::async_trait]
impl AsyncSource for ExecuteBackgroundJobSource {
    const NAME: &'static str = "execute_job";

    #[async_trait::unboxed_simple]
    #[async_backtrace::framed]
    async fn generate(&mut self) -> Result<Option<DataBlock>> {
        let background_handler = get_background_service_handler();
        background_handler
            .execute_scheduled_job(
                self.ctx.get_tenant(),
                self.ctx.get_current_user()?.identity(),
                self.job_name.clone(),
            )
            .await?;
        Ok(None)
    }
}

impl TableFunction for ExecuteBackgroundJobTable {
    fn function_name(&self) -> &str {
        self.name()
    }

    fn as_table<'a>(self: Arc<Self>) -> Arc<dyn Table + 'a>
    where Self: 'a {
        self
    }
}