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

type learnedLinearPolicy struct {
	WindowBias            int
	PendingOrdersWeight   int
	QueueDepthWeight      int
	ImbalanceWeight       int
	SpreadTightCutoff     int64
	ScoreMidCut           int
	ScoreHighCut          int
	RiskScaleHigh         float64
	RiskScaleLow          float64
	TieBreakPendingCut    int
	TieBreakImbalanceCut  int
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
	var learned *learnedLinearPolicy
	if policy == PolicyLearnedLinear {
		model := trainLearnedLinearPolicy(a.env.cfg)
		learned = &model
	}
	for !timestep.Done {
		action := a.selectAction(policy, timestep.Observation, learned)
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

func (a *Adapter) selectAction(policy PolicyController, observation Observation, learned *learnedLinearPolicy) ControlAction {
	switch policy {
	case PolicyBurstAware:
		return burstAwareAction(a.ActionSpec(), observation)
	case PolicyLearnedLinear:
		if learned == nil {
			return ControlAction{}
		}
		return learnedLinearAction(a.ActionSpec(), observation, *learned)
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
	case PolicyLearnedLinear:
		return "Policy-LearnedLinear-100-250ms"
	default:
		return base
	}
}

func trainLearnedLinearPolicy(cfg ScenarioConfig) learnedLinearPolicy {
	trainingSeeds := []int64{101, 103, 107, 109}
	candidates := []learnedLinearPolicy{
		{WindowBias: -2, PendingOrdersWeight: 1, QueueDepthWeight: 1, ImbalanceWeight: 1, SpreadTightCutoff: 1, ScoreMidCut: 10, ScoreHighCut: 18, RiskScaleHigh: 1.00, RiskScaleLow: 0.85, TieBreakPendingCut: 2, TieBreakImbalanceCut: 5},
		{WindowBias: 0, PendingOrdersWeight: 1, QueueDepthWeight: 1, ImbalanceWeight: 2, SpreadTightCutoff: 1, ScoreMidCut: 11, ScoreHighCut: 19, RiskScaleHigh: 1.05, RiskScaleLow: 0.85, TieBreakPendingCut: 2, TieBreakImbalanceCut: 4},
		{WindowBias: 2, PendingOrdersWeight: 2, QueueDepthWeight: 1, ImbalanceWeight: 2, SpreadTightCutoff: 2, ScoreMidCut: 12, ScoreHighCut: 20, RiskScaleHigh: 1.10, RiskScaleLow: 0.80, TieBreakPendingCut: 1, TieBreakImbalanceCut: 4},
		{WindowBias: 4, PendingOrdersWeight: 2, QueueDepthWeight: 2, ImbalanceWeight: 2, SpreadTightCutoff: 2, ScoreMidCut: 13, ScoreHighCut: 21, RiskScaleHigh: 1.15, RiskScaleLow: 0.80, TieBreakPendingCut: 1, TieBreakImbalanceCut: 3},
		{WindowBias: 6, PendingOrdersWeight: 3, QueueDepthWeight: 2, ImbalanceWeight: 1, SpreadTightCutoff: 2, ScoreMidCut: 14, ScoreHighCut: 22, RiskScaleHigh: 1.20, RiskScaleLow: 0.75, TieBreakPendingCut: 1, TieBreakImbalanceCut: 3},
	}

	best := candidates[0]
	bestReward := -1e18
	for _, candidate := range candidates {
		totalReward := 0.0
		for _, seed := range trainingSeeds {
			trainingCfg := cfg
			trainingCfg.Seed = seed
			adapter := NewAdapter(trainingCfg)
			timestep := adapter.Reset()
			for !timestep.Done {
				timestep = adapter.Step(learnedLinearAction(adapter.ActionSpec(), timestep.Observation, candidate))
			}
			result := adapter.env.benchmarkResult(0)
			totalReward += learnedPolicyScore(result)
		}
		if totalReward > bestReward {
			bestReward = totalReward
			best = candidate
		}
	}
	return best
}

func learnedLinearAction(spec ActionSpec, observation Observation, model learnedLinearPolicy) ControlAction {
	action := ControlAction{}
	queueDepth := observation.BuyDepth + observation.SellDepth + observation.PendingOrders
	imbalance := absInt(observation.BuyDepth - observation.SellDepth)
	score := model.WindowBias +
		model.PendingOrdersWeight*observation.PendingOrders +
		model.ImbalanceWeight*imbalance
	if observation.Spread > model.SpreadTightCutoff {
		score += model.QueueDepthWeight
	}
	if queueDepth <= 4 {
		score -= 2
	}

	if spec.SupportsBatchWindowControl {
		target := spec.MinBatchWindowSteps
		switch {
		case observation.Spread <= model.SpreadTightCutoff && queueDepth <= 4:
			target = spec.MinBatchWindowSteps
		case score >= model.ScoreHighCut:
			target = spec.MaxBatchWindowSteps
		case score >= model.ScoreMidCut:
			target = minInt(spec.MaxBatchWindowSteps, spec.MinBatchWindowSteps+10)
		default:
			target = minInt(spec.MaxBatchWindowSteps, spec.MinBatchWindowSteps+3)
		}
		action.TargetBatchWindowSteps = &target
	}
	if spec.SupportsRiskLimitScale {
		scale := model.RiskScaleLow
		if score >= model.ScoreMidCut {
			scale = model.RiskScaleHigh
		}
		if observation.RiskRejections > 0 {
			scale = maxFloat(spec.MinRiskLimitScale, model.RiskScaleLow-0.1)
		}
		action.RiskLimitScale = &scale
	}
	if spec.SupportsTieBreakToggle {
		randomize := observation.PendingOrders >= model.TieBreakPendingCut || imbalance >= model.TieBreakImbalanceCut
		action.RandomizeTieBreak = &randomize
	}
	return action
}

func learnedPolicyScore(result BenchmarkResult) float64 {
	return 0.25*result.OrdersPerSec +
		1.0*result.FillsPerSec -
		0.30*result.P99LatencyMs -
		18.0*result.AveragePriceImpact -
		220.0*absFloat(result.QueuePriorityAdvantage) -
		0.10*result.LatencyArbitrageProfit -
		3.0*float64(result.RiskRejections) -
		25.0*float64(result.ConservationBreaches)
}

func maxFloat(a, b float64) float64 {
	if a > b {
		return a
	}
	return b
}

func absFloat(v float64) float64 {
	if v < 0 {
		return -v
	}
	return v
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
