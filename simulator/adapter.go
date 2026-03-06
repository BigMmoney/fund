package simulator

import (
	"math"
	"sort"
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

type linearArmModel struct {
	Name    string
	Action  ControlAction
	A       [][]float64
	B       []float64
	Theta   []float64
	Updates int
}

type learnedLinUCBPolicy struct {
	Actions []linearArmModel
	Alpha   float64
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
	var linucb *learnedLinUCBPolicy
	if policy == PolicyLearnedLinUCB {
		model := trainLearnedLinUCBPolicy(a.env.cfg)
		linucb = &model
	}
	for !timestep.Done {
		action := a.selectAction(policy, timestep.Observation, linucb)
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

func (a *Adapter) selectAction(policy PolicyController, observation Observation, linucb *learnedLinUCBPolicy) ControlAction {
	switch policy {
	case PolicyBurstAware:
		return burstAwareAction(a.ActionSpec(), observation)
	case PolicyLearnedLinUCB:
		if linucb == nil {
			return ControlAction{}
		}
		return chooseLinUCBAction(a.ActionSpec(), observation, *linucb)
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
	case PolicyLearnedLinUCB:
		return "Policy-LearnedLinUCB-100-250ms"
	default:
		return base
	}
}

func trainLearnedLinUCBPolicy(cfg ScenarioConfig) learnedLinUCBPolicy {
	trainingSeeds := []int64{101, 103, 107, 109, 113, 127}
	spec := NewAdapter(cfg).ActionSpec()
	actions := candidateBanditActions(spec)
	featureDim := len(observationFeatures(Observation{}))
	models := make([]linearArmModel, 0, len(actions))
	for _, candidate := range actions {
		models = append(models, linearArmModel{
			Name:   candidate.Name,
			Action: candidate.Action,
			A:      identityMatrix(featureDim),
			B:      make([]float64, featureDim),
			Theta:  make([]float64, featureDim),
		})
	}
	policy := learnedLinUCBPolicy{
		Actions: models,
		Alpha:   0.55,
	}

	for _, seed := range trainingSeeds {
		trainingCfg := cfg
		trainingCfg.Seed = seed
		adapter := NewAdapter(trainingCfg)
		timestep := adapter.Reset()
		for !timestep.Done {
			features := observationFeatures(timestep.Observation)
			arm := selectLinUCBArm(policy, features)
			timestep = adapter.Step(policy.Actions[arm].Action)
			policy.Actions[arm].A = outerAdd(policy.Actions[arm].A, features)
			addScaledInPlace(policy.Actions[arm].B, features, timestep.Reward)
			policy.Actions[arm].Theta = solveLinearSystem(policy.Actions[arm].A, policy.Actions[arm].B)
			policy.Actions[arm].Updates++
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

func chooseLinUCBAction(spec ActionSpec, observation Observation, policy learnedLinUCBPolicy) ControlAction {
	if len(policy.Actions) == 0 {
		return fallbackBanditAction(spec)
	}
	features := observationFeatures(observation)
	bestIdx := selectLinUCBArm(policy, features)
	if bestIdx < 0 || bestIdx >= len(policy.Actions) {
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

func observationFeatures(observation Observation) []float64 {
	queueDepth := float64(observation.BuyDepth + observation.SellDepth + observation.PendingOrders)
	imbalance := float64(absInt(observation.BuyDepth - observation.SellDepth))
	spread := float64(observation.Spread)
	pending := float64(observation.PendingOrders)
	risk := float64(observation.RiskRejections)
	progress := 0.0
	if observation.Step > 0 {
		progress = float64(observation.Step) / 125.0
	}
	return []float64{
		1.0,
		queueDepth / 16.0,
		imbalance / 8.0,
		spread / 4.0,
		pending / 8.0,
		risk / 4.0,
		progress,
	}
}

func selectLinUCBArm(policy learnedLinUCBPolicy, features []float64) int {
	bestIdx := 0
	bestScore := math.Inf(-1)
	for idx, arm := range policy.Actions {
		invA := invertMatrix(arm.A)
		exploit := dot(arm.Theta, features)
		explore := policy.Alpha * math.Sqrt(maxFloat(quadraticForm(features, invA), 0))
		score := exploit + explore
		if arm.Updates == 0 {
			score += 1e6
		}
		if score > bestScore {
			bestScore = score
			bestIdx = idx
		}
	}
	return bestIdx
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

func dot(left, right []float64) float64 {
	sum := 0.0
	for idx := range left {
		sum += left[idx] * right[idx]
	}
	return sum
}

func identityMatrix(size int) [][]float64 {
	matrix := make([][]float64, size)
	for i := range matrix {
		matrix[i] = make([]float64, size)
		matrix[i][i] = 1.0
	}
	return matrix
}

func copyMatrix(src [][]float64) [][]float64 {
	dst := make([][]float64, len(src))
	for i := range src {
		dst[i] = append([]float64(nil), src[i]...)
	}
	return dst
}

func outerAdd(matrix [][]float64, features []float64) [][]float64 {
	out := copyMatrix(matrix)
	for i := range features {
		for j := range features {
			out[i][j] += features[i] * features[j]
		}
	}
	return out
}

func addScaledInPlace(dst, src []float64, scale float64) {
	for idx := range dst {
		dst[idx] += src[idx] * scale
	}
}

func solveLinearSystem(a [][]float64, b []float64) []float64 {
	n := len(a)
	aug := make([][]float64, n)
	for i := 0; i < n; i++ {
		aug[i] = make([]float64, n+1)
		copy(aug[i], a[i])
		aug[i][n] = b[i]
	}
	for col := 0; col < n; col++ {
		pivot := col
		for row := col + 1; row < n; row++ {
			if math.Abs(aug[row][col]) > math.Abs(aug[pivot][col]) {
				pivot = row
			}
		}
		if math.Abs(aug[pivot][col]) < 1e-9 {
			continue
		}
		aug[col], aug[pivot] = aug[pivot], aug[col]
		scale := aug[col][col]
		for j := col; j <= n; j++ {
			aug[col][j] /= scale
		}
		for row := 0; row < n; row++ {
			if row == col {
				continue
			}
			factor := aug[row][col]
			for j := col; j <= n; j++ {
				aug[row][j] -= factor * aug[col][j]
			}
		}
	}
	solution := make([]float64, n)
	for i := 0; i < n; i++ {
		solution[i] = aug[i][n]
	}
	return solution
}

func invertMatrix(a [][]float64) [][]float64 {
	n := len(a)
	aug := make([][]float64, n)
	for i := 0; i < n; i++ {
		aug[i] = make([]float64, 2*n)
		copy(aug[i], a[i])
		aug[i][n+i] = 1.0
	}
	for col := 0; col < n; col++ {
		pivot := col
		for row := col + 1; row < n; row++ {
			if math.Abs(aug[row][col]) > math.Abs(aug[pivot][col]) {
				pivot = row
			}
		}
		if math.Abs(aug[pivot][col]) < 1e-9 {
			return identityMatrix(n)
		}
		aug[col], aug[pivot] = aug[pivot], aug[col]
		scale := aug[col][col]
		for j := col; j < 2*n; j++ {
			aug[col][j] /= scale
		}
		for row := 0; row < n; row++ {
			if row == col {
				continue
			}
			factor := aug[row][col]
			for j := col; j < 2*n; j++ {
				aug[row][j] -= factor * aug[col][j]
			}
		}
	}
	out := make([][]float64, n)
	for i := 0; i < n; i++ {
		out[i] = append([]float64(nil), aug[i][n:]...)
	}
	return out
}

func quadraticForm(x []float64, matrix [][]float64) float64 {
	tmp := make([]float64, len(x))
	for i := range matrix {
		for j := range matrix[i] {
			tmp[i] += matrix[i][j] * x[j]
		}
	}
	return dot(x, tmp)
}

func maxFloat(a, b float64) float64 {
	if a > b {
		return a
	}
	return b
}

func rankedBanditModels(policy learnedLinUCBPolicy) []linearArmModel {
	models := append([]linearArmModel(nil), policy.Actions...)
	sort.Slice(models, func(i, j int) bool {
		return models[i].Updates > models[j].Updates
	})
	return models
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
