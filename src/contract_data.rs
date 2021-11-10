
use elrond_wasm::{
    api::ManagedTypeApi,
    types::{BigUint, BoxedBytes},
};

elrond_wasm::derive_imports!();

#[derive(TopEncode, TopDecode, NestedEncode, NestedDecode, PartialEq, TypeAbi, Clone)]
pub struct ScheduleType<M: ManagedTypeApi> {
    pub schedule_total_amount: BigUint<M>,
    pub schedule_is_fixed_amount: bool,
    pub schedule_percent: u8,
    pub schedule_amount: BigUint<M>,
    pub schedule_period: u64,
    pub schedule_ticks: u64
}

#[derive(NestedEncode, NestedDecode, PartialEq, TypeAbi)]
pub struct GroupType<M: ManagedTypeApi>{
    pub group_identifier: BoxedBytes,
    pub group_schedule: ScheduleType<M>,
}