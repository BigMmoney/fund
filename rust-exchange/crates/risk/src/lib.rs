use anyhow::Result;
use ledger::{LedgerService, SpotTradeSettlement};
use std::sync::Arc;
use thiserror::Error;
use types::{
    AuthenticatedPrincipal, Command, InstrumentKind, InstrumentSpec, NewOrderCommand,
    RiskCheckedCommand, RiskReserveIds, Side,
};

#[derive(Debug, Error, PartialEq, Eq)]
pub enum RiskError {
    #[error("insufficient position for reduce-only order")]
    InsufficientReduceOnlyPosition,
    #[error("risk operation failed: {0}")]
    OperationFailed(String),
}

#[derive(Clone)]
pub struct RiskEngine {
    ledger: Arc<LedgerService>,
}

#[derive(Clone)]
pub struct RiskContext {
    pub instrument: InstrumentSpec,
    pub ledger: Arc<LedgerService>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ReserveDecision {
    pub reserve_cash: i64,
    pub reserve_position: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FillIntent {
    pub buy_user_id: String,
    pub sell_user_id: String,
    pub market_id: String,
    pub outcome: i32,
    pub price: i64,
    pub amount: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SettlementDecision {
    pub use_spot_settlement: bool,
    pub use_derivative_settlement: bool,
    pub reserve_consumed_buy: i64,
    pub reserve_consumed_sell: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MarginSnapshot {
    pub user_id: String,
    pub market_id: String,
    pub outcome: i32,
    pub collateral_total: i64,
    pub position_qty: i64,
    pub mark_price: i64,
    pub notional: i64,
    pub initial_margin_required: i64,
    pub maintenance_margin_required: i64,
    pub margin_ratio_bps: Option<i64>,
    pub liquidation_required: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LiquidationCandidate {
    pub user_id: String,
    pub market_id: String,
    pub outcome: i32,
    pub position_qty: i64,
    pub mark_price: i64,
    pub collateral_total: i64,
    pub maintenance_margin_required: i64,
    pub margin_ratio_bps: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FundingPaymentPreview {
    pub user_id: String,
    pub market_id: String,
    pub outcome: i32,
    pub position_qty: i64,
    pub mark_price: i64,
    pub funding_rate_ppm: i64,
    pub signed_payment: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct LiquidationExecution {
    pub user_id: String,
    pub liquidator_user_id: String,
    pub market_id: String,
    pub outcome: i32,
    pub transferred_position_qty: i64,
    pub collateral_penalty_target: i64,
    pub collateral_penalty_paid: i64,
    pub insurance_fund_contribution: i64,
    pub socialized_loss_contribution: i64,
    pub socialized_loss_allocations: Vec<SocializedLossTransfer>,
    pub uncovered_loss: i64,
    pub bankruptcy_reference_price: Option<i64>,
    pub mark_price: i64,
    pub maintenance_margin_bps: i64,
    pub penalty_bps: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct FundingSettlement {
    pub market_id: String,
    pub outcome: i32,
    pub payer_user_id: String,
    pub receiver_user_id: String,
    pub settled_position_qty: i64,
    pub mark_price: i64,
    pub funding_rate_ppm: i64,
    pub settled_amount: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct SocializedLossTransfer {
    pub payer_user_id: String,
    pub receiver_user_id: String,
    pub market_id: String,
    pub outcome: i32,
    pub amount: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct AdlCandidate {
    pub user_id: String,
    pub market_id: String,
    pub outcome: i32,
    pub position_qty: i64,
    pub collateral_total: i64,
    pub notional: i64,
    pub effective_leverage_bps: i64,
    pub bankruptcy_distance_bps: i64,
    pub adl_score_bps: i64,
    pub bankruptcy_reference_price: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct BankruptcyPriceModel {
    pub maintenance_buffer_bps: i64,
    pub liquidation_fee_bps: i64,
    pub slippage_buffer_bps: i64,
    pub insurance_haircut_bps: i64,
}

impl Default for BankruptcyPriceModel {
    fn default() -> Self {
        Self {
            maintenance_buffer_bps: 1_000,
            liquidation_fee_bps: 500,
            slippage_buffer_bps: 100,
            insurance_haircut_bps: 200,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct BankruptcyPriceDetails {
    pub bankruptcy_reference_price: i64,
    pub maintenance_reference_price: i64,
    pub maintenance_buffer: i64,
    pub liquidation_fee_buffer: i64,
    pub slippage_buffer: i64,
    pub insurance_haircut: i64,
    pub effective_collateral: i64,
    pub bankruptcy_buffer: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct AdlGovernance {
    pub maintenance_margin_bps: i64,
    pub leverage_weight_bps: i64,
    pub bankruptcy_distance_weight_bps: i64,
    pub size_weight_bps: i64,
    pub buffer_weight_bps: i64,
    pub max_candidates: usize,
    pub max_socialized_loss_share_bps_per_candidate: i64,
}

impl Default for AdlGovernance {
    fn default() -> Self {
        Self {
            maintenance_margin_bps: 1_000,
            leverage_weight_bps: 3_500,
            bankruptcy_distance_weight_bps: 3_500,
            size_weight_bps: 1_500,
            buffer_weight_bps: 1_500,
            max_candidates: 25,
            max_socialized_loss_share_bps_per_candidate: 5_000,
        }
    }
}

pub trait RiskPolicy: Send + Sync {
    fn validate_order(&self, ctx: &RiskContext, order: &NewOrderCommand) -> Result<(), RiskError>;

    fn reserve_requirement(
        &self,
        ctx: &RiskContext,
        order: &NewOrderCommand,
    ) -> Result<ReserveDecision, RiskError>;

    fn settlement_decision(
        &self,
        ctx: &RiskContext,
        fill: &FillIntent,
        buy_leverage: Option<u32>,
        sell_leverage: Option<u32>,
    ) -> Result<SettlementDecision, RiskError>;
}

#[derive(Debug, Default)]
pub struct SpotRiskPolicy;

#[derive(Debug, Default)]
pub struct MarginRiskPolicy;

#[derive(Debug, Default)]
pub struct PerpetualRiskPolicy;

pub fn policy_for_instrument_kind(kind: InstrumentKind) -> Box<dyn RiskPolicy> {
    match kind {
        InstrumentKind::Spot => Box::new(SpotRiskPolicy),
        InstrumentKind::Margin => Box::new(MarginRiskPolicy),
        InstrumentKind::Perpetual => Box::new(PerpetualRiskPolicy),
    }
}

fn ignore_duplicate(result: anyhow::Result<()>) -> anyhow::Result<()> {
    match result {
        Ok(()) => Ok(()),
        Err(error) if error.to_string().contains("duplicate op_id") => Ok(()),
        Err(error) => Err(error),
    }
}

fn effective_leverage(leverage: Option<u32>) -> Result<u32, RiskError> {
    let value = leverage.unwrap_or(1).max(1);
    if value == 0 {
        return Err(RiskError::OperationFailed("invalid leverage".to_string()));
    }
    Ok(value)
}

fn required_margin(notional: i64, leverage: u32) -> i64 {
    if leverage <= 1 {
        return notional.max(0);
    }
    notional.max(0).saturating_div(leverage as i64)
}

impl RiskPolicy for SpotRiskPolicy {
    fn validate_order(&self, _ctx: &RiskContext, order: &NewOrderCommand) -> Result<(), RiskError> {
        if order.amount <= 0 {
            return Err(RiskError::OperationFailed(
                "amount must be positive".to_string(),
            ));
        }
        if matches!(order.side, Side::Buy | Side::Sell) && order.leverage.unwrap_or(1) > 1 {
            return Err(RiskError::OperationFailed(
                "spot orders do not support leverage".to_string(),
            ));
        }
        Ok(())
    }

    fn reserve_requirement(
        &self,
        _ctx: &RiskContext,
        order: &NewOrderCommand,
    ) -> Result<ReserveDecision, RiskError> {
        self.validate_order(_ctx, order)?;
        let price = order.price.unwrap_or(0).max(0);
        Ok(match order.side {
            Side::Buy => ReserveDecision {
                reserve_cash: price.saturating_mul(order.amount),
                reserve_position: 0,
            },
            Side::Sell => ReserveDecision {
                reserve_cash: 0,
                reserve_position: order.amount,
            },
        })
    }

    fn settlement_decision(
        &self,
        _ctx: &RiskContext,
        _fill: &FillIntent,
        _buy_leverage: Option<u32>,
        _sell_leverage: Option<u32>,
    ) -> Result<SettlementDecision, RiskError> {
        Ok(SettlementDecision {
            use_spot_settlement: true,
            use_derivative_settlement: false,
            reserve_consumed_buy: 0,
            reserve_consumed_sell: 0,
        })
    }
}

impl RiskPolicy for MarginRiskPolicy {
    fn validate_order(&self, ctx: &RiskContext, order: &NewOrderCommand) -> Result<(), RiskError> {
        if order.amount <= 0 {
            return Err(RiskError::OperationFailed(
                "amount must be positive".to_string(),
            ));
        }
        let leverage = effective_leverage(order.leverage)?;
        if let Some(max_leverage) = ctx.instrument.max_leverage {
            if leverage > max_leverage {
                return Err(RiskError::OperationFailed(
                    "leverage exceeds instrument maximum".to_string(),
                ));
            }
        }
        Ok(())
    }

    fn reserve_requirement(
        &self,
        ctx: &RiskContext,
        order: &NewOrderCommand,
    ) -> Result<ReserveDecision, RiskError> {
        self.validate_order(ctx, order)?;
        let price = order.price.unwrap_or(0).max(0);
        let notional = price.saturating_mul(order.amount.max(0));
        let margin = required_margin(notional, effective_leverage(order.leverage)?);
        Ok(ReserveDecision {
            reserve_cash: margin,
            reserve_position: 0,
        })
    }

    fn settlement_decision(
        &self,
        _ctx: &RiskContext,
        fill: &FillIntent,
        buy_leverage: Option<u32>,
        sell_leverage: Option<u32>,
    ) -> Result<SettlementDecision, RiskError> {
        let notional = fill.price.max(0).saturating_mul(fill.amount.max(0));
        Ok(SettlementDecision {
            use_spot_settlement: false,
            use_derivative_settlement: true,
            reserve_consumed_buy: required_margin(notional, effective_leverage(buy_leverage)?),
            reserve_consumed_sell: required_margin(notional, effective_leverage(sell_leverage)?),
        })
    }
}

impl RiskPolicy for PerpetualRiskPolicy {
    fn validate_order(&self, ctx: &RiskContext, order: &NewOrderCommand) -> Result<(), RiskError> {
        MarginRiskPolicy.validate_order(ctx, order)
    }

    fn reserve_requirement(
        &self,
        ctx: &RiskContext,
        order: &NewOrderCommand,
    ) -> Result<ReserveDecision, RiskError> {
        MarginRiskPolicy.reserve_requirement(ctx, order)
    }

    fn settlement_decision(
        &self,
        ctx: &RiskContext,
        fill: &FillIntent,
        buy_leverage: Option<u32>,
        sell_leverage: Option<u32>,
    ) -> Result<SettlementDecision, RiskError> {
        MarginRiskPolicy.settlement_decision(ctx, fill, buy_leverage, sell_leverage)
    }
}

impl RiskEngine {
    pub fn new(ledger: Arc<LedgerService>) -> Self {
        Self { ledger }
    }

    pub fn ledger(&self) -> Arc<LedgerService> {
        self.ledger.clone()
    }

    pub fn context_for_instrument(&self, instrument: InstrumentSpec) -> RiskContext {
        RiskContext {
            instrument,
            ledger: self.ledger.clone(),
        }
    }

    pub fn available_cash(&self, user_id: &str) -> i64 {
        self.ledger.cash_available_balance(user_id)
    }

    pub fn total_cash_collateral(&self, user_id: &str) -> i64 {
        self.ledger
            .cash_available_balance(user_id)
            .saturating_add(self.ledger.cash_hold_balance(user_id))
    }

    pub fn available_position(&self, user_id: &str, market_id: &str, outcome: i32) -> i64 {
        self.ledger
            .position_available_balance(user_id, market_id, outcome)
    }

    pub fn available_derivative_position(
        &self,
        user_id: &str,
        market_id: &str,
        outcome: i32,
    ) -> i64 {
        self.ledger
            .derivative_position_balance(user_id, market_id, outcome)
    }

    pub fn reserve_buy(&self, user_id: &str, amount: i64, op_id: &str) -> Result<RiskReserveIds> {
        ignore_duplicate(
            self.ledger
                .create_cash_hold(user_id, amount, op_id.to_string()),
        )?;
        Ok(RiskReserveIds {
            cash_op_id: Some(op_id.to_string()),
            position_op_id: None,
        })
    }

    pub fn reserve_sell(
        &self,
        user_id: &str,
        market_id: &str,
        outcome: i32,
        amount: i64,
        op_id: &str,
    ) -> Result<RiskReserveIds> {
        ignore_duplicate(self.ledger.create_position_hold(
            user_id,
            market_id,
            outcome,
            amount,
            op_id.to_string(),
        ))?;
        Ok(RiskReserveIds {
            cash_op_id: None,
            position_op_id: Some(op_id.to_string()),
        })
    }

    pub fn release_buy(&self, user_id: &str, amount: i64, op_id: &str) -> Result<()> {
        ignore_duplicate(
            self.ledger
                .release_cash_hold(user_id, amount, op_id.to_string()),
        )
    }

    pub fn release_sell(
        &self,
        user_id: &str,
        market_id: &str,
        outcome: i32,
        amount: i64,
        op_id: &str,
    ) -> Result<()> {
        ignore_duplicate(self.ledger.release_position_hold(
            user_id,
            market_id,
            outcome,
            amount,
            op_id.to_string(),
        ))
    }

    pub fn reserve_margin(
        &self,
        user_id: &str,
        amount: i64,
        op_id: &str,
    ) -> Result<RiskReserveIds> {
        ignore_duplicate(
            self.ledger
                .create_cash_hold(user_id, amount, op_id.to_string()),
        )?;
        Ok(RiskReserveIds {
            cash_op_id: Some(op_id.to_string()),
            position_op_id: None,
        })
    }

    pub fn release_margin(&self, user_id: &str, amount: i64, op_id: &str) -> Result<()> {
        ignore_duplicate(
            self.ledger
                .release_cash_hold(user_id, amount, op_id.to_string()),
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub fn settle_trade(
        &self,
        buy_user_id: &str,
        sell_user_id: &str,
        market_id: &str,
        outcome: i32,
        price: i64,
        amount: i64,
        op_id: &str,
    ) -> Result<()> {
        ignore_duplicate(self.ledger.settle_trade(SpotTradeSettlement {
            buy_user_id,
            sell_user_id,
            market_id,
            outcome,
            price,
            amount,
            op_id: op_id.to_string(),
        }))
    }

    pub fn settle_derivative_trade(
        &self,
        buy_user_id: &str,
        sell_user_id: &str,
        market_id: &str,
        outcome: i32,
        amount: i64,
        op_id: &str,
    ) -> Result<()> {
        ignore_duplicate(self.ledger.settle_derivative_trade(
            buy_user_id,
            sell_user_id,
            market_id,
            outcome,
            amount,
            op_id.to_string(),
        ))
    }

    pub fn ensure_reduce_only_sell_capacity(
        &self,
        instrument_kind: InstrumentKind,
        user_id: &str,
        market_id: &str,
        outcome: i32,
        requested_amount: i64,
        already_reserved_to_sell: i64,
    ) -> Result<(), RiskError> {
        let gross_position = match instrument_kind {
            InstrumentKind::Spot => self
                .available_position(user_id, market_id, outcome)
                .saturating_add(
                    self.ledger
                        .position_hold_balance(user_id, market_id, outcome),
                ),
            InstrumentKind::Margin | InstrumentKind::Perpetual => self
                .available_derivative_position(user_id, market_id, outcome)
                .max(0),
        };
        let remaining_capacity = gross_position.saturating_sub(already_reserved_to_sell);
        if requested_amount > remaining_capacity {
            return Err(RiskError::InsufficientReduceOnlyPosition);
        }
        Ok(())
    }

    pub fn to_risk_checked_command(
        &self,
        principal: AuthenticatedPrincipal,
        command: Command,
        reserve_ids: RiskReserveIds,
    ) -> RiskCheckedCommand {
        RiskCheckedCommand {
            command_seq: command.metadata().command_seq.unwrap_or_default(),
            reserve_ids,
            principal,
            command,
        }
    }

    pub fn maintenance_margin_requirement(
        &self,
        notional: i64,
        maintenance_margin_bps: i64,
    ) -> i64 {
        notional
            .saturating_mul(maintenance_margin_bps.max(0))
            .saturating_div(10_000)
    }

    pub fn margin_snapshot(
        &self,
        user_id: &str,
        instrument: &InstrumentSpec,
        outcome: i32,
        mark_price: i64,
        leverage: Option<u32>,
        maintenance_margin_bps: i64,
    ) -> Result<MarginSnapshot, RiskError> {
        let position_qty = match instrument.kind {
            InstrumentKind::Spot => {
                self.available_position(user_id, &instrument.instrument_id, outcome)
            }
            InstrumentKind::Margin | InstrumentKind::Perpetual => {
                self.available_derivative_position(user_id, &instrument.instrument_id, outcome)
            }
        };
        let notional = mark_price
            .abs()
            .checked_mul(position_qty.abs())
            .ok_or_else(|| {
                RiskError::OperationFailed("mark_price*position overflow".to_string())
            })?;
        let leverage = effective_leverage(leverage)?;
        let initial_margin_required = required_margin(notional, leverage);
        let maintenance_margin_required =
            self.maintenance_margin_requirement(notional, maintenance_margin_bps);
        let collateral_total = self.total_cash_collateral(user_id);
        let margin_ratio_bps = if notional > 0 {
            Some(
                collateral_total
                    .saturating_mul(10_000)
                    .saturating_div(notional),
            )
        } else {
            None
        };
        let liquidation_required =
            position_qty != 0 && collateral_total < maintenance_margin_required;

        Ok(MarginSnapshot {
            user_id: user_id.to_string(),
            market_id: instrument.instrument_id.clone(),
            outcome,
            collateral_total,
            position_qty,
            mark_price,
            notional,
            initial_margin_required,
            maintenance_margin_required,
            margin_ratio_bps,
            liquidation_required,
        })
    }

    pub fn evaluate_liquidation(
        &self,
        user_id: &str,
        instrument: &InstrumentSpec,
        outcome: i32,
        mark_price: i64,
        leverage: Option<u32>,
        maintenance_margin_bps: i64,
    ) -> Result<Option<LiquidationCandidate>, RiskError> {
        let snapshot = self.margin_snapshot(
            user_id,
            instrument,
            outcome,
            mark_price,
            leverage,
            maintenance_margin_bps,
        )?;
        if !snapshot.liquidation_required {
            return Ok(None);
        }
        Ok(Some(LiquidationCandidate {
            user_id: snapshot.user_id,
            market_id: snapshot.market_id,
            outcome: snapshot.outcome,
            position_qty: snapshot.position_qty,
            mark_price: snapshot.mark_price,
            collateral_total: snapshot.collateral_total,
            maintenance_margin_required: snapshot.maintenance_margin_required,
            margin_ratio_bps: snapshot.margin_ratio_bps,
        }))
    }

    pub fn preview_funding_payment(
        &self,
        user_id: &str,
        market_id: &str,
        outcome: i32,
        mark_price: i64,
        funding_rate_ppm: i64,
    ) -> Result<FundingPaymentPreview, RiskError> {
        let position_qty = self.available_derivative_position(user_id, market_id, outcome);
        let notional = mark_price
            .abs()
            .checked_mul(position_qty.abs())
            .ok_or_else(|| {
                RiskError::OperationFailed("mark_price*position overflow".to_string())
            })?;
        let unsigned_payment = (notional as i128)
            .saturating_mul(funding_rate_ppm as i128)
            .saturating_div(1_000_000i128);
        let signed_payment = if position_qty > 0 {
            -(unsigned_payment as i64)
        } else if position_qty < 0 {
            unsigned_payment as i64
        } else {
            0
        };
        Ok(FundingPaymentPreview {
            user_id: user_id.to_string(),
            market_id: market_id.to_string(),
            outcome,
            position_qty,
            mark_price,
            funding_rate_ppm,
            signed_payment,
        })
    }

    #[allow(clippy::too_many_arguments)]
    pub fn execute_liquidation(
        &self,
        user_id: &str,
        liquidator_user_id: &str,
        instrument: &InstrumentSpec,
        outcome: i32,
        mark_price: i64,
        leverage: Option<u32>,
        maintenance_margin_bps: i64,
        penalty_bps: i64,
        op_id_prefix: &str,
    ) -> Result<LiquidationExecution, RiskError> {
        self.execute_liquidation_with_governance(
            user_id,
            liquidator_user_id,
            instrument,
            outcome,
            mark_price,
            leverage,
            maintenance_margin_bps,
            penalty_bps,
            op_id_prefix,
            &AdlGovernance::default(),
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub fn execute_liquidation_with_governance(
        &self,
        user_id: &str,
        liquidator_user_id: &str,
        instrument: &InstrumentSpec,
        outcome: i32,
        mark_price: i64,
        leverage: Option<u32>,
        maintenance_margin_bps: i64,
        penalty_bps: i64,
        op_id_prefix: &str,
        adl_governance: &AdlGovernance,
    ) -> Result<LiquidationExecution, RiskError> {
        self.execute_partial_liquidation_with_governance(
            user_id,
            liquidator_user_id,
            instrument,
            outcome,
            mark_price,
            leverage,
            maintenance_margin_bps,
            penalty_bps,
            None,
            op_id_prefix,
            adl_governance,
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub fn execute_partial_liquidation_with_governance(
        &self,
        user_id: &str,
        liquidator_user_id: &str,
        instrument: &InstrumentSpec,
        outcome: i32,
        mark_price: i64,
        leverage: Option<u32>,
        maintenance_margin_bps: i64,
        penalty_bps: i64,
        liquidation_qty: Option<i64>,
        op_id_prefix: &str,
        adl_governance: &AdlGovernance,
    ) -> Result<LiquidationExecution, RiskError> {
        if instrument.kind == InstrumentKind::Spot {
            return Err(RiskError::OperationFailed(
                "spot instrument does not support liquidation".to_string(),
            ));
        }
        if user_id == liquidator_user_id {
            return Err(RiskError::OperationFailed(
                "liquidator must differ from liquidated user".to_string(),
            ));
        }

        let candidate = self
            .evaluate_liquidation(
                user_id,
                instrument,
                outcome,
                mark_price,
                leverage,
                maintenance_margin_bps,
            )?
            .ok_or_else(|| RiskError::OperationFailed("liquidation not required".to_string()))?;

        let candidate_qty = candidate.position_qty.abs();
        let qty = liquidation_qty.unwrap_or(candidate_qty).min(candidate_qty);
        if qty == 0 {
            return Err(RiskError::OperationFailed(
                "liquidation requires non-zero position".to_string(),
            ));
        }

        let bankruptcy_reference_price = self
            .bankruptcy_reference_price_details(user_id, instrument, outcome, mark_price)
            .map(|details| details.bankruptcy_reference_price);

        let transfer_op = format!("{op_id_prefix}:position");
        if candidate.position_qty > 0 {
            ignore_duplicate(self.settle_derivative_trade(
                liquidator_user_id,
                user_id,
                &instrument.instrument_id,
                outcome,
                qty,
                &transfer_op,
            ))
            .map_err(|error| RiskError::OperationFailed(error.to_string()))?;
        } else {
            ignore_duplicate(self.settle_derivative_trade(
                user_id,
                liquidator_user_id,
                &instrument.instrument_id,
                outcome,
                qty,
                &transfer_op,
            ))
            .map_err(|error| RiskError::OperationFailed(error.to_string()))?;
        }

        let available_cash = self.available_cash(user_id).max(0);
        let collateral_penalty_target = candidate
            .mark_price
            .abs()
            .saturating_mul(qty)
            .saturating_mul(penalty_bps.max(0))
            .saturating_div(10_000);
        let collateral_penalty_paid = available_cash.min(collateral_penalty_target);
        if collateral_penalty_paid > 0 {
            ignore_duplicate(self.ledger.transfer_cash(
                user_id,
                liquidator_user_id,
                collateral_penalty_paid,
                format!("{op_id_prefix}:cash"),
            ))
            .map_err(|error| RiskError::OperationFailed(error.to_string()))?;
        }

        let remaining_penalty = collateral_penalty_target.saturating_sub(collateral_penalty_paid);
        let insurance_fund_contribution = self
            .ledger
            .insurance_fund_balance()
            .max(0)
            .min(remaining_penalty);
        if insurance_fund_contribution > 0 {
            ignore_duplicate(self.ledger.transfer_cash_between_accounts(
                &LedgerService::insurance_fund_account(),
                &LedgerService::cash_account(liquidator_user_id),
                insurance_fund_contribution,
                format!("{op_id_prefix}:insurance"),
            ))
            .map_err(|error| RiskError::OperationFailed(error.to_string()))?;
        }
        let socialized_needed = remaining_penalty.saturating_sub(insurance_fund_contribution);
        let socialized_position_qty = if candidate.position_qty > 0 {
            qty
        } else {
            -qty
        };
        let socialized_loss_allocations = self.apply_socialized_loss_with_governance(
            &instrument.instrument_id,
            outcome,
            socialized_position_qty,
            liquidator_user_id,
            socialized_needed,
            op_id_prefix,
            adl_governance,
        )?;
        let socialized_loss_contribution: i64 = socialized_loss_allocations
            .iter()
            .map(|item| item.amount)
            .sum();
        let uncovered_loss = socialized_needed.saturating_sub(socialized_loss_contribution);

        Ok(LiquidationExecution {
            user_id: user_id.to_string(),
            liquidator_user_id: liquidator_user_id.to_string(),
            market_id: instrument.instrument_id.clone(),
            outcome,
            transferred_position_qty: qty,
            collateral_penalty_target,
            collateral_penalty_paid,
            insurance_fund_contribution,
            socialized_loss_contribution,
            socialized_loss_allocations,
            uncovered_loss,
            bankruptcy_reference_price,
            mark_price,
            maintenance_margin_bps,
            penalty_bps,
        })
    }

    #[allow(clippy::too_many_arguments)]
    pub fn settle_funding_between_users(
        &self,
        long_user_id: &str,
        short_user_id: &str,
        market_id: &str,
        outcome: i32,
        mark_price: i64,
        funding_rate_ppm: i64,
        op_id_prefix: &str,
    ) -> Result<FundingSettlement, RiskError> {
        let long_position = self.available_derivative_position(long_user_id, market_id, outcome);
        let short_position = self.available_derivative_position(short_user_id, market_id, outcome);
        if long_position <= 0 {
            return Err(RiskError::OperationFailed(
                "long_user_id must hold positive derivative position".to_string(),
            ));
        }
        if short_position >= 0 {
            return Err(RiskError::OperationFailed(
                "short_user_id must hold negative derivative position".to_string(),
            ));
        }

        let settled_position_qty = long_position.min(short_position.abs());
        if settled_position_qty == 0 || funding_rate_ppm == 0 {
            return Ok(FundingSettlement {
                market_id: market_id.to_string(),
                outcome,
                payer_user_id: long_user_id.to_string(),
                receiver_user_id: short_user_id.to_string(),
                settled_position_qty,
                mark_price,
                funding_rate_ppm,
                settled_amount: 0,
            });
        }

        let notional = mark_price
            .abs()
            .checked_mul(settled_position_qty)
            .ok_or_else(|| {
                RiskError::OperationFailed("mark_price*position overflow".to_string())
            })?;
        let settled_amount = ((notional as i128)
            .saturating_mul((funding_rate_ppm as i128).abs())
            .saturating_div(1_000_000i128)) as i64;

        let (payer_user_id, receiver_user_id) = if funding_rate_ppm > 0 {
            (long_user_id, short_user_id)
        } else {
            (short_user_id, long_user_id)
        };

        if settled_amount > 0 {
            ignore_duplicate(self.ledger.transfer_cash(
                payer_user_id,
                receiver_user_id,
                settled_amount,
                format!("{op_id_prefix}:cash"),
            ))
            .map_err(|error| RiskError::OperationFailed(error.to_string()))?;
        }

        Ok(FundingSettlement {
            market_id: market_id.to_string(),
            outcome,
            payer_user_id: payer_user_id.to_string(),
            receiver_user_id: receiver_user_id.to_string(),
            settled_position_qty,
            mark_price,
            funding_rate_ppm,
            settled_amount,
        })
    }

    pub fn liquidation_candidates(
        &self,
        user_ids: &[String],
        instrument: &InstrumentSpec,
        outcome: i32,
        mark_price: i64,
        leverage: Option<u32>,
        maintenance_margin_bps: i64,
    ) -> Result<Vec<LiquidationCandidate>, RiskError> {
        let mut candidates = Vec::new();
        for user_id in user_ids {
            if let Some(candidate) = self.evaluate_liquidation(
                user_id,
                instrument,
                outcome,
                mark_price,
                leverage,
                maintenance_margin_bps,
            )? {
                candidates.push(candidate);
            }
        }
        candidates.sort_by(|lhs, rhs| {
            lhs.margin_ratio_bps
                .unwrap_or(i64::MIN)
                .cmp(&rhs.margin_ratio_bps.unwrap_or(i64::MIN))
                .then_with(|| lhs.user_id.cmp(&rhs.user_id))
        });
        Ok(candidates)
    }

    pub fn settle_funding_batch(
        &self,
        market_id: &str,
        outcome: i32,
        mark_price: i64,
        funding_rate_ppm: i64,
        user_ids: &[String],
        op_id_prefix: &str,
    ) -> Result<Vec<FundingSettlement>, RiskError> {
        if funding_rate_ppm == 0 {
            return Ok(Vec::new());
        }

        let mut longs: Vec<(String, i64)> = user_ids
            .iter()
            .filter_map(|user_id| {
                let qty = self.available_derivative_position(user_id, market_id, outcome);
                (qty > 0).then(|| (user_id.clone(), qty))
            })
            .collect();
        let mut shorts: Vec<(String, i64)> = user_ids
            .iter()
            .filter_map(|user_id| {
                let qty = self.available_derivative_position(user_id, market_id, outcome);
                (qty < 0).then(|| (user_id.clone(), qty.abs()))
            })
            .collect();

        longs.sort_by(|lhs, rhs| lhs.0.cmp(&rhs.0));
        shorts.sort_by(|lhs, rhs| lhs.0.cmp(&rhs.0));

        let mut long_index = 0usize;
        let mut short_index = 0usize;
        let mut settlements = Vec::new();
        let mut pair_index = 0usize;

        while long_index < longs.len() && short_index < shorts.len() {
            let settled_qty = longs[long_index].1.min(shorts[short_index].1);
            if settled_qty <= 0 {
                break;
            }

            let long_user_id = longs[long_index].0.clone();
            let short_user_id = shorts[short_index].0.clone();
            let settlement = self.settle_funding_between_users(
                &long_user_id,
                &short_user_id,
                market_id,
                outcome,
                mark_price,
                funding_rate_ppm,
                &format!("{op_id_prefix}:pair-{pair_index}"),
            )?;
            settlements.push(FundingSettlement {
                settled_position_qty: settled_qty,
                ..settlement
            });

            longs[long_index].1 -= settled_qty;
            shorts[short_index].1 -= settled_qty;
            if longs[long_index].1 == 0 {
                long_index += 1;
            }
            if shorts[short_index].1 == 0 {
                short_index += 1;
            }
            pair_index += 1;
        }

        Ok(settlements)
    }

    pub fn bankruptcy_reference_price_details(
        &self,
        user_id: &str,
        instrument: &InstrumentSpec,
        outcome: i32,
        mark_price: i64,
    ) -> Option<BankruptcyPriceDetails> {
        self.bankruptcy_reference_price_details_with_model(
            user_id,
            instrument,
            outcome,
            mark_price,
            &BankruptcyPriceModel::default(),
        )
    }

    pub fn bankruptcy_reference_price_details_with_model(
        &self,
        user_id: &str,
        instrument: &InstrumentSpec,
        outcome: i32,
        mark_price: i64,
        model: &BankruptcyPriceModel,
    ) -> Option<BankruptcyPriceDetails> {
        if instrument.kind == InstrumentKind::Spot {
            return None;
        }
        let position_qty =
            self.available_derivative_position(user_id, &instrument.instrument_id, outcome);
        if position_qty == 0 {
            return None;
        }
        let qty = position_qty.abs().max(1);
        let collateral = self.total_cash_collateral(user_id).max(0);
        let mark_abs = mark_price.abs().max(1);
        let notional = mark_abs.saturating_mul(qty);
        let maintenance_buffer =
            self.maintenance_margin_requirement(notional, model.maintenance_buffer_bps.max(0));
        let liquidation_fee_buffer = notional
            .saturating_mul(model.liquidation_fee_bps.max(0))
            .saturating_div(10_000);
        let slippage_buffer = notional
            .saturating_mul(model.slippage_buffer_bps.max(0))
            .saturating_div(10_000);
        let insurance_haircut = collateral
            .saturating_mul(model.insurance_haircut_bps.max(0))
            .saturating_div(10_000);
        let effective_collateral = collateral.saturating_sub(insurance_haircut);
        let bankruptcy_buffer = effective_collateral
            .saturating_sub(maintenance_buffer)
            .saturating_sub(liquidation_fee_buffer)
            .saturating_sub(slippage_buffer);
        let maintenance_buffer_remaining = effective_collateral.saturating_sub(maintenance_buffer);
        let bankruptcy_per_contract = bankruptcy_buffer / qty;
        let maintenance_per_contract = maintenance_buffer_remaining / qty;
        let direction: i128 = if position_qty > 0 { -1 } else { 1 };
        let shift_price = |base: i64, per_contract: i64| -> i64 {
            let shifted =
                (base as i128).saturating_add(direction.saturating_mul(per_contract as i128));
            shifted.clamp(0, i64::MAX as i128) as i64
        };
        Some(BankruptcyPriceDetails {
            bankruptcy_reference_price: shift_price(mark_price, bankruptcy_per_contract),
            maintenance_reference_price: shift_price(mark_price, maintenance_per_contract),
            maintenance_buffer,
            liquidation_fee_buffer,
            slippage_buffer,
            insurance_haircut,
            effective_collateral,
            bankruptcy_buffer,
        })
    }

    pub fn bankruptcy_reference_price(
        &self,
        user_id: &str,
        instrument: &InstrumentSpec,
        outcome: i32,
        mark_price: i64,
    ) -> Option<i64> {
        self.bankruptcy_reference_price_details(user_id, instrument, outcome, mark_price)
            .map(|details| details.bankruptcy_reference_price)
    }

    pub fn adl_ranking(
        &self,
        instrument: &InstrumentSpec,
        outcome: i32,
        mark_price: i64,
        liquidated_position_qty: i64,
    ) -> Vec<AdlCandidate> {
        self.adl_ranking_with_governance(
            instrument,
            outcome,
            mark_price,
            liquidated_position_qty,
            &AdlGovernance::default(),
        )
    }

    pub fn adl_ranking_with_governance(
        &self,
        instrument: &InstrumentSpec,
        outcome: i32,
        mark_price: i64,
        liquidated_position_qty: i64,
        governance: &AdlGovernance,
    ) -> Vec<AdlCandidate> {
        if instrument.kind == InstrumentKind::Spot || liquidated_position_qty == 0 {
            return Vec::new();
        }
        let target_sign = if liquidated_position_qty > 0 { -1 } else { 1 };
        let liquidated_qty_abs = liquidated_position_qty.abs().max(1);
        let total_weight = governance
            .leverage_weight_bps
            .saturating_add(governance.bankruptcy_distance_weight_bps)
            .saturating_add(governance.size_weight_bps)
            .saturating_add(governance.buffer_weight_bps)
            .max(1);
        let mut items: Vec<_> =
            self.ledger
                .user_ids()
                .into_iter()
                .filter_map(|user_id| {
                    let position_qty = self.available_derivative_position(
                        &user_id,
                        &instrument.instrument_id,
                        outcome,
                    );
                    let opposite = (target_sign < 0 && position_qty < 0)
                        || (target_sign > 0 && position_qty > 0);
                    if !opposite || position_qty == 0 {
                        return None;
                    }
                    let collateral_total = self.total_cash_collateral(&user_id).max(0);
                    let notional = mark_price.abs().saturating_mul(position_qty.abs());
                    let maintenance_margin_required = self.maintenance_margin_requirement(
                        notional,
                        governance.maintenance_margin_bps.max(0),
                    );
                    let excess_collateral =
                        collateral_total.saturating_sub(maintenance_margin_required);
                    let effective_leverage_bps = if collateral_total > 0 {
                        notional
                            .saturating_mul(10_000)
                            .saturating_div(collateral_total)
                    } else {
                        i64::MAX / 4
                    };
                    let bankruptcy_reference_price =
                        self.bankruptcy_reference_price(&user_id, instrument, outcome, mark_price);
                    let bankruptcy_distance_bps = bankruptcy_reference_price
                        .map(|reference| {
                            mark_price
                                .saturating_sub(reference)
                                .abs()
                                .saturating_mul(10_000)
                                .saturating_div(mark_price.abs().max(1))
                        })
                        .unwrap_or(i64::MAX / 4)
                        .max(1);
                    let inverse_bankruptcy_score_bps =
                        1_000_000i64.saturating_div(bankruptcy_distance_bps.clamp(1, 1_000_000));
                    let size_score_bps = position_qty
                        .abs()
                        .saturating_mul(10_000)
                        .saturating_div(liquidated_qty_abs);
                    let buffer_pressure_score_bps = if excess_collateral > 0 {
                        notional
                            .saturating_mul(10_000)
                            .saturating_div(excess_collateral)
                    } else {
                        i64::MAX / 4
                    };
                    let weighted_score = (effective_leverage_bps as i128)
                        .saturating_mul(governance.leverage_weight_bps.max(0) as i128)
                        .saturating_add((inverse_bankruptcy_score_bps as i128).saturating_mul(
                            governance.bankruptcy_distance_weight_bps.max(0) as i128,
                        ))
                        .saturating_add(
                            (size_score_bps as i128)
                                .saturating_mul(governance.size_weight_bps.max(0) as i128),
                        )
                        .saturating_add(
                            (buffer_pressure_score_bps as i128)
                                .saturating_mul(governance.buffer_weight_bps.max(0) as i128),
                        )
                        .saturating_div(total_weight as i128)
                        .clamp(0, i64::MAX as i128) as i64;
                    Some(AdlCandidate {
                        user_id: user_id.clone(),
                        market_id: instrument.instrument_id.clone(),
                        outcome,
                        position_qty,
                        collateral_total,
                        notional,
                        effective_leverage_bps,
                        bankruptcy_distance_bps,
                        adl_score_bps: weighted_score,
                        bankruptcy_reference_price,
                    })
                })
                .collect();
        items.sort_by(|lhs, rhs| {
            rhs.adl_score_bps
                .cmp(&lhs.adl_score_bps)
                .then_with(|| rhs.effective_leverage_bps.cmp(&lhs.effective_leverage_bps))
                .then_with(|| {
                    lhs.bankruptcy_distance_bps
                        .cmp(&rhs.bankruptcy_distance_bps)
                })
                .then_with(|| rhs.notional.cmp(&lhs.notional))
                .then_with(|| lhs.user_id.cmp(&rhs.user_id))
        });
        if items.len() > governance.max_candidates {
            items.truncate(governance.max_candidates);
        }
        items
    }

    pub fn apply_socialized_loss(
        &self,
        market_id: &str,
        outcome: i32,
        liquidated_position_qty: i64,
        receiver_user_id: &str,
        uncovered_loss: i64,
        op_id_prefix: &str,
    ) -> Result<Vec<SocializedLossTransfer>, RiskError> {
        self.apply_socialized_loss_with_governance(
            market_id,
            outcome,
            liquidated_position_qty,
            receiver_user_id,
            uncovered_loss,
            op_id_prefix,
            &AdlGovernance::default(),
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub fn apply_socialized_loss_with_governance(
        &self,
        market_id: &str,
        outcome: i32,
        liquidated_position_qty: i64,
        receiver_user_id: &str,
        mut uncovered_loss: i64,
        op_id_prefix: &str,
        governance: &AdlGovernance,
    ) -> Result<Vec<SocializedLossTransfer>, RiskError> {
        if uncovered_loss <= 0 || liquidated_position_qty == 0 {
            return Ok(Vec::new());
        }

        let instrument = InstrumentSpec {
            instrument_id: market_id.to_string(),
            kind: InstrumentKind::Perpetual,
            quote_asset: "USDC".to_string(),
            margin_mode: None,
            max_leverage: None,
            tick_size: 1,
            lot_size: 1,
            price_band_bps: 0,
            risk_policy_id: "adl".to_string(),
        };
        let adl_candidates = self
            .adl_ranking_with_governance(
                &instrument,
                outcome,
                1,
                liquidated_position_qty,
                governance,
            )
            .into_iter()
            .filter(|item| item.user_id != receiver_user_id)
            .collect::<Vec<_>>();
        if adl_candidates.is_empty() {
            return Ok(Vec::new());
        }

        let original_uncovered = uncovered_loss;
        let mut transfers = Vec::new();
        for candidate in adl_candidates {
            if uncovered_loss <= 0 {
                break;
            }
            let payer_user_id = candidate.user_id;
            let available_cash = self.available_cash(&payer_user_id).max(0);
            let per_candidate_cap = if governance.max_socialized_loss_share_bps_per_candidate > 0 {
                ((original_uncovered as i128)
                    .saturating_mul(governance.max_socialized_loss_share_bps_per_candidate as i128)
                    .saturating_div(10_000))
                .clamp(1, i64::MAX as i128) as i64
            } else {
                0
            };
            let mut amount = available_cash.min(uncovered_loss);
            if per_candidate_cap > 0 {
                amount = amount.min(per_candidate_cap);
            }
            if amount <= 0 {
                continue;
            }
            ignore_duplicate(self.ledger.transfer_cash(
                &payer_user_id,
                receiver_user_id,
                amount,
                format!("{}:socialized:{}", op_id_prefix, transfers.len()),
            ))
            .map_err(|error| RiskError::OperationFailed(error.to_string()))?;
            transfers.push(SocializedLossTransfer {
                payer_user_id,
                receiver_user_id: receiver_user_id.to_string(),
                market_id: market_id.to_string(),
                outcome,
                amount,
            });
            uncovered_loss = uncovered_loss.saturating_sub(amount);
        }
        Ok(transfers)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use eventbus::EventBus;
    use std::sync::Arc;
    use types::{CommandMetadata, InstrumentKind, MarginMode, TimeInForce};

    fn test_instrument(kind: InstrumentKind) -> InstrumentSpec {
        InstrumentSpec {
            instrument_id: match kind {
                InstrumentKind::Spot => "btc-usdt".to_string(),
                InstrumentKind::Margin => "margin:btc-usdt".to_string(),
                InstrumentKind::Perpetual => "perp:btc-usdt".to_string(),
            },
            kind,
            quote_asset: "USDC".to_string(),
            margin_mode: match kind {
                InstrumentKind::Spot => None,
                InstrumentKind::Margin | InstrumentKind::Perpetual => Some(MarginMode::Isolated),
            },
            max_leverage: match kind {
                InstrumentKind::Spot => None,
                InstrumentKind::Margin | InstrumentKind::Perpetual => Some(10),
            },
            tick_size: 1,
            lot_size: 1,
            price_band_bps: 1_000,
            risk_policy_id: "test".to_string(),
        }
    }

    fn test_order(market_id: &str, leverage: Option<u32>) -> NewOrderCommand {
        NewOrderCommand {
            metadata: CommandMetadata::new("req-1"),
            client_order_id: "order-1".to_string(),
            user_id: "u-1".to_string(),
            session_id: None,
            market_id: market_id.to_string(),
            side: Side::Buy,
            order_type: types::OrderType::Limit,
            time_in_force: TimeInForce::Gtc,
            price: Some(100),
            amount: 5,
            outcome: 0,
            post_only: false,
            reduce_only: false,
            leverage,
            expires_at: None,
        }
    }

    #[test]
    fn spot_policy_rejects_leverage() {
        let engine = RiskEngine::new(Arc::new(LedgerService::new(EventBus::new())));
        let ctx = engine.context_for_instrument(test_instrument(InstrumentKind::Spot));
        let policy = SpotRiskPolicy;
        let error = policy
            .validate_order(&ctx, &test_order("btc-usdt", Some(3)))
            .unwrap_err();
        assert!(matches!(error, RiskError::OperationFailed(_)));
    }

    #[test]
    fn margin_policy_computes_initial_margin() {
        let engine = RiskEngine::new(Arc::new(LedgerService::new(EventBus::new())));
        let ctx = engine.context_for_instrument(test_instrument(InstrumentKind::Margin));
        let policy = MarginRiskPolicy;
        let reserve = policy
            .reserve_requirement(&ctx, &test_order("margin:btc-usdt", Some(5)))
            .unwrap();
        assert_eq!(reserve.reserve_cash, 100);
        assert_eq!(reserve.reserve_position, 0);
    }

    #[test]
    fn perpetual_policy_uses_derivative_settlement() {
        let engine = RiskEngine::new(Arc::new(LedgerService::new(EventBus::new())));
        let ctx = engine.context_for_instrument(test_instrument(InstrumentKind::Perpetual));
        let policy = PerpetualRiskPolicy;
        let decision = policy
            .settlement_decision(
                &ctx,
                &FillIntent {
                    buy_user_id: "b".to_string(),
                    sell_user_id: "s".to_string(),
                    market_id: "perp:btc-usdt".to_string(),
                    outcome: 0,
                    price: 100,
                    amount: 5,
                },
                Some(5),
                Some(5),
            )
            .unwrap();
        assert!(decision.use_derivative_settlement);
        assert!(!decision.use_spot_settlement);
        assert_eq!(decision.reserve_consumed_buy, 100);
        assert_eq!(decision.reserve_consumed_sell, 100);
    }

    #[test]
    fn margin_snapshot_detects_liquidation_threshold() {
        let ledger = Arc::new(LedgerService::new(EventBus::new()));
        ledger
            .process_deposit("u1", 50, "dep-1".to_string())
            .unwrap();
        ledger
            .settle_derivative_trade("u1", "u2", "perp:btc-usdt", 0, 10, "deriv-1".to_string())
            .unwrap();
        let engine = RiskEngine::new(ledger);

        let snapshot = engine
            .margin_snapshot(
                "u1",
                &test_instrument(InstrumentKind::Perpetual),
                0,
                100,
                Some(5),
                1000,
            )
            .unwrap();

        assert_eq!(snapshot.position_qty, 10);
        assert_eq!(snapshot.notional, 1000);
        assert!(snapshot.liquidation_required);
    }

    #[test]
    fn positive_funding_rate_charges_long_and_pays_short() {
        let ledger = Arc::new(LedgerService::new(EventBus::new()));
        ledger
            .settle_derivative_trade(
                "long",
                "short",
                "perp:btc-usdt",
                0,
                10,
                "deriv-1".to_string(),
            )
            .unwrap();
        let engine = RiskEngine::new(ledger);

        let long_payment = engine
            .preview_funding_payment("long", "perp:btc-usdt", 0, 100, 10_000)
            .unwrap();
        let short_payment = engine
            .preview_funding_payment("short", "perp:btc-usdt", 0, 100, 10_000)
            .unwrap();

        assert!(long_payment.signed_payment < 0);
        assert!(short_payment.signed_payment > 0);
    }

    #[test]
    fn liquidation_exec_transfers_position_and_collateral() {
        let ledger = Arc::new(LedgerService::new(EventBus::new()));
        ledger
            .process_deposit("u1", 50, "dep-u1".to_string())
            .unwrap();
        ledger
            .settle_derivative_trade("u1", "maker", "perp:btc-usdt", 0, 10, "deriv-1".to_string())
            .unwrap();
        let engine = RiskEngine::new(ledger.clone());

        let execution = engine
            .execute_liquidation(
                "u1",
                "liq",
                &test_instrument(InstrumentKind::Perpetual),
                0,
                100,
                Some(5),
                1_000,
                500,
                "liq-op-1",
            )
            .unwrap();

        assert_eq!(execution.transferred_position_qty, 10);
        assert_eq!(execution.collateral_penalty_target, 50);
        assert_eq!(execution.collateral_penalty_paid, 50);
        assert_eq!(execution.insurance_fund_contribution, 0);
        assert_eq!(execution.socialized_loss_contribution, 0);
        assert!(execution.bankruptcy_reference_price.is_some());
        assert_eq!(execution.uncovered_loss, 0);
        assert_eq!(
            ledger.derivative_position_balance("u1", "perp:btc-usdt", 0),
            0
        );
        assert_eq!(
            ledger.derivative_position_balance("liq", "perp:btc-usdt", 0),
            10
        );
        assert!(ledger.cash_available_balance("liq") > 0);
    }

    #[test]
    fn liquidation_can_draw_from_insurance_fund_for_shortfall() {
        let ledger = Arc::new(LedgerService::new(EventBus::new()));
        ledger
            .deposit_insurance_fund(100, "if-dep-liq-1".to_string())
            .unwrap();
        ledger
            .process_deposit("u1", 10, "dep-u1-short".to_string())
            .unwrap();
        ledger
            .settle_derivative_trade(
                "u1",
                "maker",
                "perp:btc-usdt",
                0,
                10,
                "deriv-short-1".to_string(),
            )
            .unwrap();
        let engine = RiskEngine::new(ledger.clone());

        let execution = engine
            .execute_liquidation(
                "u1",
                "liq",
                &test_instrument(InstrumentKind::Perpetual),
                0,
                100,
                Some(5),
                1_000,
                500,
                "liq-op-if-1",
            )
            .unwrap();

        assert_eq!(execution.collateral_penalty_target, 50);
        assert_eq!(execution.collateral_penalty_paid, 10);
        assert_eq!(execution.insurance_fund_contribution, 40);
        assert_eq!(execution.socialized_loss_contribution, 0);
        assert_eq!(execution.uncovered_loss, 0);
        assert_eq!(ledger.insurance_fund_balance(), 60);
        assert_eq!(ledger.cash_available_balance("liq"), 50);
    }

    #[test]
    fn adl_ranking_orders_highest_leverage_first() {
        let ledger = Arc::new(LedgerService::new(EventBus::new()));
        ledger
            .process_deposit("s1", 10, "dep-adl-s1".to_string())
            .unwrap();
        ledger
            .process_deposit("s2", 100, "dep-adl-s2".to_string())
            .unwrap();
        ledger
            .settle_derivative_trade(
                "maker",
                "s1",
                "perp:btc-usdt",
                0,
                10,
                "adl-deriv-1".to_string(),
            )
            .unwrap();
        ledger
            .settle_derivative_trade(
                "maker",
                "s2",
                "perp:btc-usdt",
                0,
                10,
                "adl-deriv-2".to_string(),
            )
            .unwrap();
        let engine = RiskEngine::new(ledger);

        let ranking = engine.adl_ranking(&test_instrument(InstrumentKind::Perpetual), 0, 100, 10);

        assert_eq!(ranking.len(), 2);
        assert_eq!(ranking[0].user_id, "s1");
        assert!(ranking[0].adl_score_bps >= ranking[1].adl_score_bps);
    }

    #[test]
    fn liquidation_can_apply_socialized_loss_after_insurance() {
        let ledger = Arc::new(LedgerService::new(EventBus::new()));
        ledger
            .deposit_insurance_fund(10, "if-dep-liq-2".to_string())
            .unwrap();
        ledger
            .process_deposit("u1", 10, "dep-u1-social".to_string())
            .unwrap();
        ledger
            .process_deposit("short1", 30, "dep-short1-social".to_string())
            .unwrap();
        ledger
            .process_deposit("short2", 30, "dep-short2-social".to_string())
            .unwrap();
        ledger
            .settle_derivative_trade(
                "u1",
                "maker",
                "perp:btc-usdt",
                0,
                10,
                "deriv-social-1".to_string(),
            )
            .unwrap();
        ledger
            .settle_derivative_trade(
                "maker",
                "short1",
                "perp:btc-usdt",
                0,
                5,
                "deriv-social-2".to_string(),
            )
            .unwrap();
        ledger
            .settle_derivative_trade(
                "maker",
                "short2",
                "perp:btc-usdt",
                0,
                5,
                "deriv-social-3".to_string(),
            )
            .unwrap();
        let engine = RiskEngine::new(ledger.clone());

        let execution = engine
            .execute_liquidation(
                "u1",
                "liq",
                &test_instrument(InstrumentKind::Perpetual),
                0,
                100,
                Some(5),
                1_000,
                500,
                "liq-op-social-1",
            )
            .unwrap();

        assert_eq!(execution.collateral_penalty_target, 50);
        assert_eq!(execution.collateral_penalty_paid, 10);
        assert_eq!(execution.insurance_fund_contribution, 10);
        assert_eq!(execution.socialized_loss_contribution, 30);
        assert_eq!(execution.uncovered_loss, 0);
        assert!(!execution.socialized_loss_allocations.is_empty());
        assert_eq!(
            execution
                .socialized_loss_allocations
                .iter()
                .map(|item| item.amount)
                .sum::<i64>(),
            30
        );
        assert_eq!(ledger.cash_available_balance("liq"), 50);
    }

    #[test]
    fn funding_settlement_moves_cash_between_counterparties() {
        let ledger = Arc::new(LedgerService::new(EventBus::new()));
        ledger
            .process_deposit("long", 100, "dep-long".to_string())
            .unwrap();
        ledger
            .process_deposit("short", 100, "dep-short".to_string())
            .unwrap();
        ledger
            .settle_derivative_trade(
                "long",
                "short",
                "perp:btc-usdt",
                0,
                10,
                "deriv-1".to_string(),
            )
            .unwrap();
        let engine = RiskEngine::new(ledger.clone());

        let settlement = engine
            .settle_funding_between_users(
                "long",
                "short",
                "perp:btc-usdt",
                0,
                100,
                10_000,
                "funding-op-1",
            )
            .unwrap();

        assert_eq!(settlement.payer_user_id, "long");
        assert_eq!(settlement.receiver_user_id, "short");
        assert_eq!(settlement.settled_amount, 10);
        assert_eq!(ledger.cash_available_balance("long"), 90);
        assert_eq!(ledger.cash_available_balance("short"), 110);
    }

    #[test]
    fn liquidation_candidates_returns_only_underwater_users() {
        let ledger = Arc::new(LedgerService::new(EventBus::new()));
        ledger
            .process_deposit("healthy", 500, "dep-healthy".to_string())
            .unwrap();
        ledger
            .process_deposit("u1", 50, "dep-u1".to_string())
            .unwrap();
        ledger
            .settle_derivative_trade(
                "healthy",
                "maker",
                "perp:btc-usdt",
                0,
                1,
                "deriv-healthy".to_string(),
            )
            .unwrap();
        ledger
            .settle_derivative_trade(
                "u1",
                "maker",
                "perp:btc-usdt",
                0,
                10,
                "deriv-u1".to_string(),
            )
            .unwrap();
        let engine = RiskEngine::new(ledger);

        let candidates = engine
            .liquidation_candidates(
                &["healthy".to_string(), "u1".to_string()],
                &test_instrument(InstrumentKind::Perpetual),
                0,
                100,
                Some(5),
                1_000,
            )
            .unwrap();

        assert_eq!(candidates.len(), 1);
        assert_eq!(candidates[0].user_id, "u1");
    }

    #[test]
    fn funding_batch_pairs_longs_and_shorts() {
        let ledger = Arc::new(LedgerService::new(EventBus::new()));
        for (user, amount) in [("l1", 100), ("l2", 100), ("s1", 100), ("s2", 100)] {
            ledger
                .process_deposit(user, amount, format!("dep-{user}"))
                .unwrap();
        }
        ledger
            .settle_derivative_trade("l1", "s1", "perp:btc-usdt", 0, 10, "deriv-1".to_string())
            .unwrap();
        ledger
            .settle_derivative_trade("l2", "s2", "perp:btc-usdt", 0, 5, "deriv-2".to_string())
            .unwrap();
        let engine = RiskEngine::new(ledger.clone());

        let settlements = engine
            .settle_funding_batch(
                "perp:btc-usdt",
                0,
                100,
                10_000,
                &[
                    "l1".to_string(),
                    "l2".to_string(),
                    "s1".to_string(),
                    "s2".to_string(),
                ],
                "fund-batch-1",
            )
            .unwrap();

        assert_eq!(settlements.len(), 2);
        assert_eq!(ledger.cash_available_balance("l1"), 90);
        assert_eq!(ledger.cash_available_balance("l2"), 95);
        assert_eq!(ledger.cash_available_balance("s1"), 110);
        assert_eq!(ledger.cash_available_balance("s2"), 105);
    }

    #[test]
    fn bankruptcy_reference_price_details_include_fee_and_buffer_haircuts() {
        let ledger = Arc::new(LedgerService::new(EventBus::new()));
        ledger
            .process_deposit("u1", 100, "dep-bankruptcy-1".to_string())
            .unwrap();
        ledger
            .settle_derivative_trade(
                "u1",
                "maker",
                "perp:btc-usdt",
                0,
                10,
                "deriv-bankruptcy-1".to_string(),
            )
            .unwrap();
        let engine = RiskEngine::new(ledger);

        let details = engine
            .bankruptcy_reference_price_details(
                "u1",
                &test_instrument(InstrumentKind::Perpetual),
                0,
                100,
            )
            .expect("details");

        assert!(details.bankruptcy_reference_price >= 0);
        assert!(details.maintenance_reference_price >= 0);
        assert!(details.effective_collateral < 100);
        assert!(details.liquidation_fee_buffer > 0);
        assert!(details.slippage_buffer > 0);
        assert!(details.insurance_haircut > 0);
    }

    #[test]
    fn governed_socialized_loss_caps_single_counterparty_burden() {
        let ledger = Arc::new(LedgerService::new(EventBus::new()));
        ledger
            .process_deposit("short1", 100, "dep-short1-cap".to_string())
            .unwrap();
        ledger
            .process_deposit("short2", 100, "dep-short2-cap".to_string())
            .unwrap();
        ledger
            .settle_derivative_trade(
                "maker",
                "short1",
                "perp:btc-usdt",
                0,
                10,
                "deriv-cap-1".to_string(),
            )
            .unwrap();
        ledger
            .settle_derivative_trade(
                "maker",
                "short2",
                "perp:btc-usdt",
                0,
                10,
                "deriv-cap-2".to_string(),
            )
            .unwrap();
        let engine = RiskEngine::new(ledger.clone());
        let governance = AdlGovernance {
            max_socialized_loss_share_bps_per_candidate: 5_000,
            max_candidates: 10,
            ..AdlGovernance::default()
        };

        let transfers = engine
            .apply_socialized_loss_with_governance(
                "perp:btc-usdt",
                0,
                10,
                "liq",
                100,
                "social-cap-1",
                &governance,
            )
            .unwrap();

        assert_eq!(transfers.len(), 2);
        assert!(transfers.iter().all(|item| item.amount <= 50));
        assert_eq!(transfers.iter().map(|item| item.amount).sum::<i64>(), 100);
    }
}
