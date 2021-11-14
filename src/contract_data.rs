
use elrond_wasm::{
    api::ManagedTypeApi,
    types::BigUint
};

elrond_wasm::derive_imports!();

#[derive(TopEncode, TopDecode, NestedEncode, NestedDecode, PartialEq, TypeAbi, Clone)]
pub struct ScheduleType<M: ManagedTypeApi> {
    pub group_total_amount: BigUint<M>,     // total number of tokens released for this group
    pub is_fixed_amount: bool,              // true if tokens are released by a fix amount each period, false if they are released by a certain percent each period
    pub group_unlock_percent: u8,           // unlock percentage for each period based on total group amount (group_unlock_percent * release_ticks must be equal to 100%)
    pub period_unlock_amount: BigUint<M>,   // fixed unlock amount for each period for all users (period_unlock_amount * release_ticks must be equal to group_total_amount)
    pub release_period: u64,                // the duration of each release period
    pub release_ticks: u64                  // total number of unlock periods
}
