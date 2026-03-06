package simulator

import "time"

type ControlAction struct {
	TargetBatchWindowSteps *int     `json:"target_batch_window_steps,omitempty"`
	RiskLimitScale         *float64 `json:"risk_limit_scale,omitempty"`
	RandomizeTieBreak      *bool    `json:"randomize_tie_break,omitempty"`
}

type ActionSpec struct {
	SupportsBatchWindowControl bool    `json:"supports_batch_window_control"`
	MinBatchWindowSteps        int     `json:"min_batch_window_steps"`
	MaxBatchWindowSteps        int     `json:"max_batch_window_steps"`
	SupportsRiskLimitScale     bool    `json:"supports_risk_limit_scale"`
	MinRiskLimitScale          float64 `json:"min_risk_limit_scale"`
	MaxRiskLimitScale          float64 `json:"max_risk_limit_scale"`
	SupportsTieBreakToggle     bool    `json:"supports_tie_break_toggle"`
}

type RewardWeights struct {
	FillWeight           float64 `json:"fill_weight"`
	SpreadPenalty        float64 `json:"spread_penalty"`
	ArbitragePenalty     float64 `json:"arbitrage_penalty"`
	RiskRejectPenalty    float64 `json:"risk_reject_penalty"`
	ConservationPenalty  float64 `json:"conservation_penalty"`
}

type MetricsDelta struct {
	FillsDelta                int     `json:"fills_delta"`
	SpreadDelta               float64 `json:"spread_delta"`
	ArbitrageProfitDelta      float64 `json:"arbitrage_profit_delta"`
	RiskRejectionsDelta       int     `json:"risk_rejections_delta"`
	ConservationBreachesDelta int     `json:"conservation_breaches_delta"`
}

type AdapterInfo struct {
	ScenarioName         string        `json:"scenario_name"`
	AppliedAction        ControlAction `json:"applied_action"`
	ActionSpec           ActionSpec    `json:"action_spec"`
	MetricsDelta         MetricsDelta  `json:"metrics_delta"`
	CurrentBatchWindowMs int           `json:"current_batch_window_ms"`
	CurrentRiskScale     float64       `json:"current_risk_scale"`
	RandomTieBreak       bool          `json:"random_tie_break"`
}

type AdapterTimestep struct {
	Observation Observation     `json:"observation"`
	Metrics     MetricsSnapshot `json:"metrics"`
	Reward      float64         `json:"reward"`
	Done        bool            `json:"done"`
	Info        AdapterInfo     `json:"info"`
}

type Adapter struct {
	env           *Environment
	rewardWeights RewardWeights
	prevMetrics   MetricsSnapshot
}

func NewAdapter(cfg ScenarioConfig) *Adapter {
	env := NewEnvironment(cfg)
	return &Adapter{
		env:           env,
		rewardWeights: defaultRewardWeights(),
		prevMetrics:   env.Metrics(),
	}
}

func defaultRewardWeights() RewardWeights {
	return RewardWeights{
		FillWeight:          1.0,
		SpreadPenalty:       0.25,
		ArbitragePenalty:    0.005,
		RiskRejectPenalty:   0.5,
		ConservationPenalty: 10.0,
	}
}

func (a *Adapter) Reset() AdapterTimestep {
	observation := a.env.Reset()
	metrics := a.env.Metrics()
	a.prevMetrics = metrics
	return AdapterTimestep{
		Observation: observation,
		Metrics:     metrics,
		Reward:      0,
		Done:        observation.Done,
		Info:        a.buildInfo(ControlAction{}, observation, metrics, MetricsDelta{}),
	}
}

func (a *Adapter) Step(action ControlAction) AdapterTimestep {
	appliedAction := a.applyAction(action)
	result := a.env.Step()
	delta := metricsDelta(a.prevMetrics, result.Metrics)
	reward := a.computeReward(delta)
	a.prevMetrics = result.Metrics
	return AdapterTimestep{
		Observation: result.Observation,
		Metrics:     result.Metrics,
		Reward:      reward,
		Done:        result.Observation.Done,
		Info:        a.buildInfo(appliedAction, result.Observation, result.Metrics, delta),
	}
}

func (a *Adapter) Observe() AdapterTimestep {
	observation := a.env.Observe()
	metrics := a.env.Metrics()
	return AdapterTimestep{
		Observation: observation,
		Metrics:     metrics,
		Reward:      0,
		Done:        observation.Done,
		Info:        a.buildInfo(ControlAction{}, observation, metrics, MetricsDelta{}),
	}
}

func (a *Adapter) ActionSpec() ActionSpec {
	spec := ActionSpec{
		SupportsRiskLimitScale: true,
		MinRiskLimitScale:      0.5,
		MaxRiskLimitScale:      1.5,
		SupportsTieBreakToggle: a.env.cfg.Mode == ModeBatch || a.env.cfg.Mode == ModeAdaptiveBatch,
	}
	if a.env.cfg.Mode == ModeAdaptiveBatch {
		spec.SupportsBatchWindowControl = true
		spec.MinBatchWindowSteps = maxInt(1, a.env.cfg.AdaptiveMinWindowSteps)
		spec.MaxBatchWindowSteps = maxInt(spec.MinBatchWindowSteps, a.env.cfg.AdaptiveMaxWindowSteps)
	}
	return spec
}

func (a *Adapter) RunPolicy(policy PolicyController) BenchmarkResult {
	start := time.Now()
	timestep := a.Reset()
	for !timestep.Done {
		action := a.selectAction(policy, timestep.Observation)
		timestep = a.Step(action)
	}
	result := a.env.benchmarkResult(time.Since(start))
	result.Name = policyScenarioName(result.Name, policy)
	return result
}

func (a *Adapter) buildInfo(applied ControlAction, observation Observation, metrics MetricsSnapshot, delta MetricsDelta) AdapterInfo {
	return AdapterInfo{
		ScenarioName:         a.env.cfg.Name,
		AppliedAction:        applied,
		ActionSpec:           a.ActionSpec(),
		MetricsDelta:         delta,
		CurrentBatchWindowMs: observation.CurrentBatchWindowStep * int(a.env.cfg.StepDuration.Milliseconds()),
		CurrentRiskScale:     a.env.runtimeRiskScale,
		RandomTieBreak:       a.env.runtimeRandomTieBreak,
	}
}

func (a *Adapter) applyAction(action ControlAction) ControlAction {
	spec := a.ActionSpec()
	applied := ControlAction{}

	if spec.SupportsBatchWindowControl && action.TargetBatchWindowSteps != nil {
		target := *action.TargetBatchWindowSteps
		if target < spec.MinBatchWindowSteps {
			target = spec.MinBatchWindowSteps
		}
		if target > spec.MaxBatchWindowSteps {
			target = spec.MaxBatchWindowSteps
		}
		a.env.currentBatchWindow = target
		applied.TargetBatchWindowSteps = &target
	}

	if spec.SupportsRiskLimitScale && action.RiskLimitScale != nil {
		scale := *action.RiskLimitScale
		if scale < spec.MinRiskLimitScale {
			scale = spec.MinRiskLimitScale
		}
		if scale > spec.MaxRiskLimitScale {
			scale = spec.MaxRiskLimitScale
		}
		a.env.runtimeRiskScale = scale
		applied.RiskLimitScale = &scale
	}

	if spec.SupportsTieBreakToggle && action.RandomizeTieBreak != nil {
		randomize := *action.RandomizeTieBreak
		a.env.runtimeRandomTieBreak = randomize
		applied.RandomizeTieBreak = &randomize
	}

	return applied
}

func (a *Adapter) selectAction(policy PolicyController, observation Observation) ControlAction {
	switch policy {
	case PolicyBurstAware:
		return burstAwareAction(a.ActionSpec(), observation)
	default:
		return ControlAction{}
	}
}

func burstAwareAction(spec ActionSpec, observation Observation) ControlAction {
	action := ControlAction{}
	if spec.SupportsBatchWindowControl {
		target := spec.MinBatchWindowSteps
		queueDepth := observation.BuyDepth + observation.SellDepth + observation.PendingOrders
		imbalance := absInt(observation.BuyDepth - observation.SellDepth)
		switch {
		case queueDepth >= 12 || imbalance >= 6:
			target = spec.MaxBatchWindowSteps
		case queueDepth >= 8 || observation.OrdersAccepted-observation.Fills >= 4:
			target = minInt(spec.MaxBatchWindowSteps, spec.MinBatchWindowSteps+10)
		case observation.Spread <= 1 && queueDepth <= 3:
			target = spec.MinBatchWindowSteps
		default:
			target = minInt(spec.MaxBatchWindowSteps, spec.MinBatchWindowSteps+5)
		}
		action.TargetBatchWindowSteps = &target
	}
	if spec.SupportsRiskLimitScale {
		scale := 1.0
		queueDepth := observation.BuyDepth + observation.SellDepth + observation.PendingOrders
		switch {
		case queueDepth >= 12:
			scale = 1.25
		case observation.RiskRejections > 0:
			scale = 0.9
		case observation.Spread <= 1 && queueDepth <= 3:
			scale = 0.8
		}
		action.RiskLimitScale = &scale
	}
	if spec.SupportsTieBreakToggle {
		randomize := observation.PendingOrders > 0 || absInt(observation.BuyDepth-observation.SellDepth) >= 5
		action.RandomizeTieBreak = &randomize
	}
	return action
}

func policyScenarioName(base string, policy PolicyController) string {
	switch policy {
	case PolicyBurstAware:
		return "Policy-BurstAware-100-250ms"
	default:
		return base
	}
}

func (a *Adapter) computeReward(delta MetricsDelta) float64 {
	return a.rewardWeights.FillWeight*float64(delta.FillsDelta) -
		a.rewardWeights.SpreadPenalty*delta.SpreadDelta -
		a.rewardWeights.ArbitragePenalty*delta.ArbitrageProfitDelta -
		a.rewardWeights.RiskRejectPenalty*float64(delta.RiskRejectionsDelta) -
		a.rewardWeights.ConservationPenalty*float64(delta.ConservationBreachesDelta)
}

func metricsDelta(prev, next MetricsSnapshot) MetricsDelta {
	return MetricsDelta{
		FillsDelta:                next.Fills - prev.Fills,
		SpreadDelta:               next.AverageSpread - prev.AverageSpread,
		ArbitrageProfitDelta:      next.LatencyArbitrageProfit - prev.LatencyArbitrageProfit,
		RiskRejectionsDelta:       next.RiskRejections - prev.RiskRejections,
		ConservationBreachesDelta: next.ConservationBreaches - prev.ConservationBreaches,
	}
}
