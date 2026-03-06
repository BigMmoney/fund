package simulator

import (
	"fmt"
	"math"
	"time"
)

type ControlAction struct {
	TargetBatchWindowSteps *int     `json:"target_batch_window_steps,omitempty"`
	RiskLimitScale         *float64 `json:"risk_limit_scale,omitempty"`
	RandomizeTieBreak      *bool    `json:"randomize_tie_break,omitempty"`
	ReleaseCadenceSteps    *int     `json:"release_cadence_steps,omitempty"`
	PriceAggressionBias    *int64   `json:"price_aggression_bias,omitempty"`
}

type ActionSpec struct {
	SupportsBatchWindowControl  bool    `json:"supports_batch_window_control"`
	MinBatchWindowSteps         int     `json:"min_batch_window_steps"`
	MaxBatchWindowSteps         int     `json:"max_batch_window_steps"`
	SupportsRiskLimitScale      bool    `json:"supports_risk_limit_scale"`
	MinRiskLimitScale           float64 `json:"min_risk_limit_scale"`
	MaxRiskLimitScale           float64 `json:"max_risk_limit_scale"`
	SupportsTieBreakToggle      bool    `json:"supports_tie_break_toggle"`
	SupportsReleaseCadence      bool    `json:"supports_release_cadence_control"`
	MinReleaseCadenceSteps      int     `json:"min_release_cadence_steps"`
	MaxReleaseCadenceSteps      int     `json:"max_release_cadence_steps"`
	SupportsPriceAggressionBias bool    `json:"supports_price_aggression_control"`
	MinPriceAggressionBias      int64   `json:"min_price_aggression_bias"`
	MaxPriceAggressionBias      int64   `json:"max_price_aggression_bias"`
}

type RewardWeights struct {
	FillWeight          float64 `json:"fill_weight"`
	SpreadPenalty       float64 `json:"spread_penalty"`
	PriceImpactPenalty  float64 `json:"price_impact_penalty"`
	QueuePenalty        float64 `json:"queue_penalty"`
	ArbitragePenalty    float64 `json:"arbitrage_penalty"`
	RiskRejectPenalty   float64 `json:"risk_reject_penalty"`
	ConservationPenalty float64 `json:"conservation_penalty"`
}

type MetricsDelta struct {
	FillsDelta                int     `json:"fills_delta"`
	SpreadDelta               float64 `json:"spread_delta"`
	PriceImpactDelta          float64 `json:"price_impact_delta"`
	QueuePriorityDelta        float64 `json:"queue_priority_delta"`
	ArbitrageProfitDelta      float64 `json:"arbitrage_profit_delta"`
	RiskRejectionsDelta       int     `json:"risk_rejections_delta"`
	ConservationBreachesDelta int     `json:"conservation_breaches_delta"`
}

type AdapterInfo struct {
	ScenarioName           string        `json:"scenario_name"`
	AppliedAction          ControlAction `json:"applied_action"`
	ActionSpec             ActionSpec    `json:"action_spec"`
	MetricsDelta           MetricsDelta  `json:"metrics_delta"`
	CurrentBatchWindowMs   int           `json:"current_batch_window_ms"`
	CurrentRiskScale       float64       `json:"current_risk_scale"`
	RandomTieBreak         bool          `json:"random_tie_break"`
	CurrentReleaseCadenceMs int          `json:"current_release_cadence_ms"`
	CurrentPriceAggression int64         `json:"current_price_aggression_bias"`
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

type learnedActionCandidate struct {
	Name   string
	Action ControlAction
}

type banditArmStats struct {
	Count     int
	RewardSum float64
}

type learnedBanditPolicy struct {
	Actions []learnedActionCandidate
	Stats   map[string][]banditArmStats
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
		PriceImpactPenalty:  1.5,
		QueuePenalty:        12.0,
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
		SupportsRiskLimitScale:      true,
		MinRiskLimitScale:           0.5,
		MaxRiskLimitScale:           1.5,
		SupportsTieBreakToggle:      a.env.cfg.Mode == ModeBatch || a.env.cfg.Mode == ModeAdaptiveBatch,
		SupportsPriceAggressionBias: true,
		MinPriceAggressionBias:      -2,
		MaxPriceAggressionBias:      2,
	}
	if a.env.cfg.Mode == ModeAdaptiveBatch {
		spec.SupportsBatchWindowControl = true
		spec.MinBatchWindowSteps = maxInt(1, a.env.cfg.AdaptiveMinWindowSteps)
		spec.MaxBatchWindowSteps = maxInt(spec.MinBatchWindowSteps, a.env.cfg.AdaptiveMaxWindowSteps)
	}
	if a.env.cfg.Mode == ModeBatch || a.env.cfg.Mode == ModeAdaptiveBatch || a.env.cfg.Mode == ModeSpeedBump {
		spec.SupportsReleaseCadence = true
		spec.MinReleaseCadenceSteps = 0
		switch a.env.cfg.Mode {
		case ModeSpeedBump:
			spec.MaxReleaseCadenceSteps = maxInt(10, maxInt(1, a.env.cfg.SpeedBumpSteps)*2)
		case ModeAdaptiveBatch:
			spec.MaxReleaseCadenceSteps = maxInt(spec.MaxBatchWindowSteps+10, spec.MinBatchWindowSteps)
		default:
			spec.MaxReleaseCadenceSteps = maxInt(10, maxInt(1, a.env.cfg.BatchWindowSteps)+10)
		}
	}
	return spec
}

func (a *Adapter) RunPolicy(policy PolicyController) BenchmarkResult {
	start := time.Now()
	timestep := a.Reset()
	var bandit *learnedBanditPolicy
	if policy == PolicyLearnedBandit {
		model := trainLearnedBanditPolicy(a.env.cfg)
		bandit = &model
	}
	for !timestep.Done {
		action := a.selectAction(policy, timestep.Observation, bandit)
		timestep = a.Step(action)
	}
	result := a.env.benchmarkResult(time.Since(start))
	result.Name = policyScenarioName(result.Name, policy)
	return result
}

func (a *Adapter) buildInfo(applied ControlAction, observation Observation, metrics MetricsSnapshot, delta MetricsDelta) AdapterInfo {
	return AdapterInfo{
		ScenarioName:            a.env.cfg.Name,
		AppliedAction:           applied,
		ActionSpec:              a.ActionSpec(),
		MetricsDelta:            delta,
		CurrentBatchWindowMs:    observation.CurrentBatchWindowStep * int(a.env.cfg.StepDuration.Milliseconds()),
		CurrentRiskScale:        a.env.runtimeRiskScale,
		RandomTieBreak:          a.env.runtimeRandomTieBreak,
		CurrentReleaseCadenceMs: observation.CurrentReleaseCadence * int(a.env.cfg.StepDuration.Milliseconds()),
		CurrentPriceAggression:  observation.CurrentPriceAggression,
	}
}

func (a *Adapter) applyAction(action ControlAction) ControlAction {
	spec := a.ActionSpec()
	applied := ControlAction{}

	if spec.SupportsBatchWindowControl && action.TargetBatchWindowSteps != nil {
		target := clampInt(*action.TargetBatchWindowSteps, spec.MinBatchWindowSteps, spec.MaxBatchWindowSteps)
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

	if spec.SupportsReleaseCadence && action.ReleaseCadenceSteps != nil {
		cadence := clampInt(*action.ReleaseCadenceSteps, spec.MinReleaseCadenceSteps, spec.MaxReleaseCadenceSteps)
		a.env.runtimeReleaseCadence = cadence
		applied.ReleaseCadenceSteps = &cadence
	}

	if spec.SupportsPriceAggressionBias && action.PriceAggressionBias != nil {
		bias := clampActionInt64(*action.PriceAggressionBias, spec.MinPriceAggressionBias, spec.MaxPriceAggressionBias)
		a.env.runtimePriceAggression = bias
		applied.PriceAggressionBias = &bias
	}

	return applied
}

func (a *Adapter) selectAction(policy PolicyController, observation Observation, bandit *learnedBanditPolicy) ControlAction {
	switch policy {
	case PolicyBurstAware:
		return burstAwareAction(a.ActionSpec(), observation)
	case PolicyLearnedBandit:
		if bandit == nil {
			return ControlAction{}
		}
		return chooseBanditAction(a.ActionSpec(), observation, *bandit)
	default:
		return ControlAction{}
	}
}

func burstAwareAction(spec ActionSpec, observation Observation) ControlAction {
	action := ControlAction{}
	queueDepth := observation.BuyDepth + observation.SellDepth + observation.PendingOrders
	imbalance := absInt(observation.BuyDepth - observation.SellDepth)

	if spec.SupportsBatchWindowControl {
		target := spec.MinBatchWindowSteps
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
		randomize := observation.PendingOrders > 0 || imbalance >= 5
		action.RandomizeTieBreak = &randomize
	}
	if spec.SupportsReleaseCadence {
		cadence := 0
		if queueDepth >= 10 || observation.PendingOrders >= 3 {
			cadence = minInt(spec.MaxReleaseCadenceSteps, maxInt(spec.MinBatchWindowSteps, spec.MinBatchWindowSteps+5))
		}
		action.ReleaseCadenceSteps = &cadence
	}
	if spec.SupportsPriceAggressionBias {
		var bias int64
		switch {
		case observation.RiskRejections > 0:
			bias = -1
		case observation.Spread <= 1 && queueDepth <= 4:
			bias = 1
		default:
			bias = 0
		}
		action.PriceAggressionBias = &bias
	}
	return action
}

func policyScenarioName(base string, policy PolicyController) string {
	switch policy {
	case PolicyBurstAware:
		return "Policy-BurstAware-100-250ms"
	case PolicyLearnedBandit:
		return "Policy-LearnedBandit-100-250ms"
	default:
		return base
	}
}

func trainLearnedBanditPolicy(cfg ScenarioConfig) learnedBanditPolicy {
	trainingSeeds := []int64{101, 103, 107, 109, 113, 127}
	spec := NewAdapter(cfg).ActionSpec()
	actions := candidateBanditActions(spec)
	policy := learnedBanditPolicy{
		Actions: actions,
		Stats:   make(map[string][]banditArmStats),
	}

	for _, seed := range trainingSeeds {
		trainingCfg := cfg
		trainingCfg.Seed = seed
		adapter := NewAdapter(trainingCfg)
		timestep := adapter.Reset()
		for !timestep.Done {
			key := banditContextKey(timestep.Observation)
			arm := selectBanditArm(policy, key)
			timestep = adapter.Step(policy.Actions[arm].Action)
			stats := policy.ensureStats(key)
			stats[arm].Count++
			stats[arm].RewardSum += timestep.Reward
			policy.Stats[key] = stats
		}
	}

	return policy
}

func candidateBanditActions(spec ActionSpec) []learnedActionCandidate {
	minWindow := spec.MinBatchWindowSteps
	midWindow := minInt(spec.MaxBatchWindowSteps, spec.MinBatchWindowSteps+10)
	maxWindow := spec.MaxBatchWindowSteps
	releaseMid := minInt(spec.MaxReleaseCadenceSteps, maxInt(0, minWindow+5))
	releaseMax := minInt(spec.MaxReleaseCadenceSteps, maxWindow)

	return []learnedActionCandidate{
		{
			Name: "fast_passive",
			Action: makeAction(
				&minWindow,
				floatPtr(0.85),
				boolPtr(false),
				intPtr(0),
				int64Ptr(-1),
			),
		},
		{
			Name: "balanced_mid",
			Action: makeAction(
				&midWindow,
				floatPtr(1.0),
				boolPtr(true),
				intPtr(releaseMid),
				int64Ptr(0),
			),
		},
		{
			Name: "fair_delay",
			Action: makeAction(
				&maxWindow,
				floatPtr(0.95),
				boolPtr(true),
				intPtr(releaseMax),
				int64Ptr(0),
			),
		},
		{
			Name: "aggressive_fast",
			Action: makeAction(
				intPtr(minInt(spec.MaxBatchWindowSteps, spec.MinBatchWindowSteps+3)),
				floatPtr(1.10),
				boolPtr(true),
				intPtr(0),
				int64Ptr(1),
			),
		},
		{
			Name: "latency_tail_guard",
			Action: makeAction(
				intPtr(minInt(spec.MaxBatchWindowSteps, spec.MinBatchWindowSteps+5)),
				floatPtr(0.95),
				boolPtr(false),
				intPtr(releaseMid),
				int64Ptr(0),
			),
		},
		{
			Name: "pressure_release",
			Action: makeAction(
				&midWindow,
				floatPtr(1.05),
				boolPtr(true),
				intPtr(releaseMax),
				int64Ptr(1),
			),
		},
	}
}

func makeAction(window *int, risk *float64, tie *bool, cadence *int, price *int64) ControlAction {
	return ControlAction{
		TargetBatchWindowSteps: window,
		RiskLimitScale:         risk,
		RandomizeTieBreak:      tie,
		ReleaseCadenceSteps:    cadence,
		PriceAggressionBias:    price,
	}
}

func banditContextKey(observation Observation) string {
	queueDepth := observation.BuyDepth + observation.SellDepth + observation.PendingOrders
	queueBucket := bucketize(queueDepth, 4, 8, 12)
	imbalanceBucket := bucketize(absInt(observation.BuyDepth-observation.SellDepth), 1, 4, 7)
	pendingBucket := bucketize(observation.PendingOrders, 0, 2, 5)
	spreadBucket := 0
	if observation.Spread > 1 {
		spreadBucket = 1
	}
	riskBucket := 0
	if observation.RiskRejections > 0 {
		riskBucket = 1
	}
	return fmt.Sprintf("q%d_i%d_p%d_s%d_r%d", queueBucket, imbalanceBucket, pendingBucket, spreadBucket, riskBucket)
}

func selectBanditArm(policy learnedBanditPolicy, key string) int {
	stats := policy.ensureStats(key)
	total := 0
	for _, arm := range stats {
		total += arm.Count
	}
	bestIdx := 0
	bestScore := -1e18
	for idx, arm := range stats {
		if arm.Count == 0 {
			return idx
		}
		mean := arm.RewardSum / float64(arm.Count)
		bonus := 1.25 * math.Sqrt(math.Log(float64(total+1))/float64(arm.Count))
		score := mean + bonus
		if score > bestScore {
			bestScore = score
			bestIdx = idx
		}
	}
	return bestIdx
}

func chooseBanditAction(spec ActionSpec, observation Observation, policy learnedBanditPolicy) ControlAction {
	key := banditContextKey(observation)
	stats, ok := policy.Stats[key]
	if !ok || len(stats) != len(policy.Actions) {
		return fallbackBanditAction(spec)
	}
	bestIdx := 0
	bestMean := -1e18
	for idx, arm := range stats {
		if arm.Count == 0 {
			continue
		}
		mean := arm.RewardSum / float64(arm.Count)
		if mean > bestMean {
			bestMean = mean
			bestIdx = idx
		}
	}
	if bestMean == -1e18 {
		return fallbackBanditAction(spec)
	}
	return policy.Actions[bestIdx].Action
}

func fallbackBanditAction(spec ActionSpec) ControlAction {
	mid := minInt(spec.MaxBatchWindowSteps, spec.MinBatchWindowSteps+10)
	cadence := minInt(spec.MaxReleaseCadenceSteps, maxInt(0, spec.MinBatchWindowSteps+5))
	return makeAction(
		&mid,
		floatPtr(1.0),
		boolPtr(true),
		intPtr(cadence),
		int64Ptr(0),
	)
}

func (p learnedBanditPolicy) ensureStats(key string) []banditArmStats {
	stats, ok := p.Stats[key]
	if ok && len(stats) == len(p.Actions) {
		return stats
	}
	stats = make([]banditArmStats, len(p.Actions))
	p.Stats[key] = stats
	return stats
}

func bucketize(v int, low, mid, high int) int {
	switch {
	case v <= low:
		return 0
	case v <= mid:
		return 1
	case v <= high:
		return 2
	default:
		return 3
	}
}

func clampInt(v, minV, maxV int) int {
	if v < minV {
		return minV
	}
	if v > maxV {
		return maxV
	}
	return v
}

func clampActionInt64(v, minV, maxV int64) int64 {
	if v < minV {
		return minV
	}
	if v > maxV {
		return maxV
	}
	return v
}

func intPtr(v int) *int { return &v }

func int64Ptr(v int64) *int64 { return &v }

func floatPtr(v float64) *float64 { return &v }

func boolPtr(v bool) *bool { return &v }

func absFloat(v float64) float64 {
	if v < 0 {
		return -v
	}
	return v
}

func (a *Adapter) computeReward(delta MetricsDelta) float64 {
	return a.rewardWeights.FillWeight*float64(delta.FillsDelta) -
		a.rewardWeights.SpreadPenalty*delta.SpreadDelta -
		a.rewardWeights.PriceImpactPenalty*delta.PriceImpactDelta -
		a.rewardWeights.QueuePenalty*absFloat(delta.QueuePriorityDelta) -
		a.rewardWeights.ArbitragePenalty*delta.ArbitrageProfitDelta -
		a.rewardWeights.RiskRejectPenalty*float64(delta.RiskRejectionsDelta) -
		a.rewardWeights.ConservationPenalty*float64(delta.ConservationBreachesDelta)
}

func metricsDelta(prev, next MetricsSnapshot) MetricsDelta {
	return MetricsDelta{
		FillsDelta:                next.Fills - prev.Fills,
		SpreadDelta:               next.AverageSpread - prev.AverageSpread,
		PriceImpactDelta:          next.AveragePriceImpact - prev.AveragePriceImpact,
		QueuePriorityDelta:        next.QueuePriorityAdvantage - prev.QueuePriorityAdvantage,
		ArbitrageProfitDelta:      next.LatencyArbitrageProfit - prev.LatencyArbitrageProfit,
		RiskRejectionsDelta:       next.RiskRejections - prev.RiskRejections,
		ConservationBreachesDelta: next.ConservationBreaches - prev.ConservationBreaches,
	}
}
