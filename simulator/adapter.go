package simulator

import (
	"encoding/json"
	"math"
	"math/rand"
	"sort"
	"sync"
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
	RetailSurplusWeight float64 `json:"retail_surplus_weight"`
	AdversePenalty      float64 `json:"adverse_penalty"`
	WelfarePenalty      float64 `json:"welfare_penalty"`
	SurplusGapPenalty   float64 `json:"surplus_gap_penalty"`
	RiskRejectPenalty   float64 `json:"risk_reject_penalty"`
	ConservationPenalty float64 `json:"conservation_penalty"`
}

type MetricsDelta struct {
	FillsDelta                int     `json:"fills_delta"`
	SpreadDelta               float64 `json:"spread_delta"`
	PriceImpactDelta          float64 `json:"price_impact_delta"`
	QueuePriorityDelta        float64 `json:"queue_priority_delta"`
	ArbitrageProfitDelta      float64 `json:"arbitrage_profit_delta"`
	RetailSurplusDelta        float64 `json:"retail_surplus_delta"`
	RetailAdverseDelta        float64 `json:"retail_adverse_delta"`
	WelfareDispersionDelta    float64 `json:"welfare_dispersion_delta"`
	SurplusTransferGapDelta   float64 `json:"surplus_transfer_gap_delta"`
	RiskRejectionsDelta       int     `json:"risk_rejections_delta"`
	ConservationBreachesDelta int     `json:"conservation_breaches_delta"`
}

type AdapterInfo struct {
	ScenarioName            string        `json:"scenario_name"`
	AppliedAction           ControlAction `json:"applied_action"`
	ActionSpec              ActionSpec    `json:"action_spec"`
	MetricsDelta            MetricsDelta  `json:"metrics_delta"`
	CurrentBatchWindowMs    int           `json:"current_batch_window_ms"`
	CurrentRiskScale        float64       `json:"current_risk_scale"`
	RandomTieBreak          bool          `json:"random_tie_break"`
	CurrentReleaseCadenceMs int           `json:"current_release_cadence_ms"`
	CurrentPriceAggression  int64         `json:"current_price_aggression_bias"`
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

type tinyMLPModel struct {
	Actions    []learnedActionCandidate
	InputDim   int
	HiddenDim  int
	W1         [][]float64
	B1         []float64
	W2         [][]float64
	B2         []float64
	TrainScore float64
}

type learnedOfflineContextualPolicy struct {
	Actions []linearArmModel
	Gamma   float64
}

type learnedFittedQPolicy struct {
	Actions        []linearArmModel
	Gamma          float64
	Iterations     int
	TrainingSeeds  []int64
	HeldOutSeeds   []int64
	HeldOutRegimes []string
}

type fittedQTrainingSnapshot struct {
	Iteration  int
	BellmanMSE float64
	Policy     learnedFittedQPolicy
}

type policyStepSample struct {
	features []float64
	hidden   []float64
	probs    []float64
	action   int
	reward   float64
}

type offlinePolicySample struct {
	features []float64
	action   int
	target   float64
}

type offlineTransitionSample struct {
	features     []float64
	action       int
	reward       float64
	nextFeatures []float64
	done         bool
}

var learnedPolicyCache = struct {
	sync.Mutex
	linucb  map[string]learnedLinUCBPolicy
	tiny    map[string]tinyMLPModel
	offline map[string]learnedOfflineContextualPolicy
	fittedQ map[string]learnedFittedQPolicy
}{
	linucb:  make(map[string]learnedLinUCBPolicy),
	tiny:    make(map[string]tinyMLPModel),
	offline: make(map[string]learnedOfflineContextualPolicy),
	fittedQ: make(map[string]learnedFittedQPolicy),
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
		RetailSurplusWeight: 18.0,
		AdversePenalty:      10.0,
		WelfarePenalty:      2.0,
		SurplusGapPenalty:   3.0,
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
	var tiny *tinyMLPModel
	var offline *learnedOfflineContextualPolicy
	var fittedQ *learnedFittedQPolicy
	if policy == PolicyLearnedLinUCB {
		model := cachedLinUCBPolicy(a.env.cfg)
		linucb = &model
	} else if policy == PolicyLearnedTinyMLP {
		model := cachedTinyMLPPolicy(a.env.cfg)
		tiny = &model
	} else if policy == PolicyLearnedOfflineContextual {
		model := cachedOfflineContextualPolicy(a.env.cfg)
		offline = &model
	} else if policy == PolicyLearnedFittedQ {
		model := cachedFittedQPolicy(a.env.cfg)
		fittedQ = &model
	}
	for !timestep.Done {
		action := a.selectAction(policy, timestep.Observation, linucb, tiny, offline, fittedQ)
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

func (a *Adapter) selectAction(policy PolicyController, observation Observation, linucb *learnedLinUCBPolicy, tiny *tinyMLPModel, offline *learnedOfflineContextualPolicy, fittedQ *learnedFittedQPolicy) ControlAction {
	switch policy {
	case PolicyBurstAware:
		return burstAwareAction(a.ActionSpec(), observation)
	case PolicyLearnedLinUCB:
		if linucb == nil {
			return ControlAction{}
		}
		return chooseLinUCBAction(a.ActionSpec(), observation, *linucb)
	case PolicyLearnedTinyMLP:
		if tiny == nil {
			return ControlAction{}
		}
		return chooseTinyMLPAction(a.ActionSpec(), observation, *tiny)
	case PolicyLearnedOfflineContextual:
		if offline == nil {
			return ControlAction{}
		}
		return chooseOfflineContextualAction(a.ActionSpec(), observation, *offline)
	case PolicyLearnedFittedQ:
		if fittedQ == nil {
			return ControlAction{}
		}
		return chooseFittedQAction(a.ActionSpec(), observation, *fittedQ)
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
	case PolicyLearnedTinyMLP:
		return "Policy-LearnedTinyMLP-100-250ms"
	case PolicyLearnedOfflineContextual:
		return "Policy-LearnedOfflineContextual-100-250ms"
	case PolicyLearnedFittedQ:
		return "Policy-LearnedFittedQ-100-250ms"
	default:
		return base
	}
}

func cachedLinUCBPolicy(cfg ScenarioConfig) learnedLinUCBPolicy {
	key := policyCacheKey(cfg, PolicyLearnedLinUCB)
	learnedPolicyCache.Lock()
	if cached, ok := learnedPolicyCache.linucb[key]; ok {
		learnedPolicyCache.Unlock()
		return cached
	}
	learnedPolicyCache.Unlock()

	trained := trainLearnedLinUCBPolicy(cfg)

	learnedPolicyCache.Lock()
	learnedPolicyCache.linucb[key] = trained
	learnedPolicyCache.Unlock()
	return trained
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

func cachedTinyMLPPolicy(cfg ScenarioConfig) tinyMLPModel {
	key := policyCacheKey(cfg, PolicyLearnedTinyMLP)
	learnedPolicyCache.Lock()
	if cached, ok := learnedPolicyCache.tiny[key]; ok {
		learnedPolicyCache.Unlock()
		return cached
	}
	learnedPolicyCache.Unlock()

	trained := trainTinyMLPPolicy(cfg)

	learnedPolicyCache.Lock()
	learnedPolicyCache.tiny[key] = trained
	learnedPolicyCache.Unlock()
	return trained
}

func cachedOfflineContextualPolicy(cfg ScenarioConfig) learnedOfflineContextualPolicy {
	key := policyCacheKey(cfg, PolicyLearnedOfflineContextual)
	learnedPolicyCache.Lock()
	if cached, ok := learnedPolicyCache.offline[key]; ok {
		learnedPolicyCache.Unlock()
		return cached
	}
	learnedPolicyCache.Unlock()

	trained := trainOfflineContextualPolicy(cfg)

	learnedPolicyCache.Lock()
	learnedPolicyCache.offline[key] = trained
	learnedPolicyCache.Unlock()
	return trained
}

func cachedFittedQPolicy(cfg ScenarioConfig) learnedFittedQPolicy {
	key := policyCacheKey(cfg, PolicyLearnedFittedQ)
	learnedPolicyCache.Lock()
	if cached, ok := learnedPolicyCache.fittedQ[key]; ok {
		learnedPolicyCache.Unlock()
		return cached
	}
	learnedPolicyCache.Unlock()

	trained := trainLearnedFittedQPolicy(cfg)

	learnedPolicyCache.Lock()
	learnedPolicyCache.fittedQ[key] = trained
	learnedPolicyCache.Unlock()
	return trained
}

func trainTinyMLPPolicy(cfg ScenarioConfig) tinyMLPModel {
	trainingSeeds := []int64{101, 103, 107, 109, 113, 127}
	spec := NewAdapter(cfg).ActionSpec()
	actions := candidateBanditActions(spec)
	inputDim := len(observationFeatures(Observation{}))
	hiddenDim := 8
	rng := rand.New(rand.NewSource(trainingRandomSeed(cfg)))
	model := initTinyMLPModel(actions, inputDim, hiddenDim, rng)
	supervisedFeatures, supervisedLabels := collectBurstAwareDataset(cfg, actions, trainingSeeds)
	trainTinyMLPSupervised(&model, supervisedFeatures, supervisedLabels, 24, 0.035, 1e-4)
	trainTinyMLPPolicyGradient(cfg, &model, trainingSeeds, rng, 36, 0.012, 0.97, 0.001)
	model.TrainScore = evaluateTinyMLPModel(cfg, model, trainingSeeds)
	return model
}

func trainOfflineContextualPolicy(cfg ScenarioConfig) learnedOfflineContextualPolicy {
	trainingSeeds := []int64{131, 137, 139, 149, 151, 157}
	spec := NewAdapter(cfg).ActionSpec()
	actions := candidateBanditActions(spec)
	featureDim := len(observationFeatures(Observation{}))
	models := make([]linearArmModel, 0, len(actions))
	for _, candidate := range actions {
		identity := identityMatrix(featureDim)
		for i := range identity {
			identity[i][i] = 1.5
		}
		models = append(models, linearArmModel{
			Name:   candidate.Name,
			Action: candidate.Action,
			A:      identity,
			B:      make([]float64, featureDim),
			Theta:  make([]float64, featureDim),
		})
	}
	policy := learnedOfflineContextualPolicy{
		Actions: models,
		Gamma:   0.97,
	}

	linucb := cachedLinUCBPolicy(cfg)
	tiny := cachedTinyMLPPolicy(cfg)
	rng := rand.New(rand.NewSource(trainingRandomSeed(cfg) + 211))
	for _, seed := range trainingSeeds {
		for _, behavior := range []PolicyController{PolicyBurstAware, PolicyLearnedLinUCB, PolicyLearnedTinyMLP} {
			samples := collectOfflineTrajectory(cfg, seed, behavior, actions, &linucb, &tiny, rng, policy.Gamma)
			updateOfflineContextualPolicy(&policy, samples)
		}
		for rollout := 0; rollout < 2; rollout++ {
			samples := collectOfflineRandomTrajectory(cfg, seed+int64(rollout), actions, rng, policy.Gamma)
			updateOfflineContextualPolicy(&policy, samples)
		}
	}
	return policy
}

func trainLearnedFittedQPolicy(cfg ScenarioConfig) learnedFittedQPolicy {
	trace := trainLearnedFittedQPolicyTrace(cfg)
	return trace[len(trace)-1].Policy
}

func trainLearnedFittedQPolicyTrace(cfg ScenarioConfig) []fittedQTrainingSnapshot {
	trainingSeeds := []int64{181, 191, 193, 197, 199, 211}
	heldOutSeeds := []int64{223, 227, 229, 233}
	spec := NewAdapter(cfg).ActionSpec()
	actions := candidateBanditActions(spec)
	featureDim := len(observationFeatures(Observation{}))
	basePolicy := learnedFittedQPolicy{
		Actions:       initLinearArmModels(actions, featureDim, 2.0),
		Gamma:         0.97,
		Iterations:    8,
		TrainingSeeds: append([]int64(nil), trainingSeeds...),
		HeldOutSeeds:  append([]int64(nil), heldOutSeeds...),
		HeldOutRegimes: []string{
			"HeldOut-HighArbWideMaker",
			"HeldOut-RetailBurst",
			"HeldOut-InformedWide",
			"HeldOut-CompositeStress",
		},
	}

	linucb := cachedLinUCBPolicy(cfg)
	tiny := cachedTinyMLPPolicy(cfg)
	offline := cachedOfflineContextualPolicy(cfg)
	rng := rand.New(rand.NewSource(trainingRandomSeed(cfg) + 503))

	transitions := make([]offlineTransitionSample, 0, len(trainingSeeds)*cfg.TotalSteps*6)
	for _, seed := range trainingSeeds {
		for _, behavior := range []PolicyController{PolicyBurstAware, PolicyLearnedLinUCB, PolicyLearnedTinyMLP, PolicyLearnedOfflineContextual} {
			transitions = append(transitions, collectOfflineTransitionTrajectory(cfg, seed, behavior, actions, &linucb, &tiny, &offline, rng)...)
		}
		for rollout := 0; rollout < 2; rollout++ {
			transitions = append(transitions, collectOfflineRandomTransitionTrajectory(cfg, seed+int64(rollout), actions, rng)...)
		}
	}

	prevModels := copyLinearArmModels(basePolicy.Actions)
	trace := make([]fittedQTrainingSnapshot, 0, basePolicy.Iterations+1)
	trace = append(trace, fittedQTrainingSnapshot{
		Iteration:  0,
		BellmanMSE: 0,
		Policy: learnedFittedQPolicy{
			Actions:        copyLinearArmModels(prevModels),
			Gamma:          basePolicy.Gamma,
			Iterations:     basePolicy.Iterations,
			TrainingSeeds:  append([]int64(nil), basePolicy.TrainingSeeds...),
			HeldOutSeeds:   append([]int64(nil), basePolicy.HeldOutSeeds...),
			HeldOutRegimes: append([]string(nil), basePolicy.HeldOutRegimes...),
		},
	})
	for iter := 0; iter < basePolicy.Iterations; iter++ {
		models := initLinearArmModels(actions, featureDim, 2.0)
		sumSquaredError := 0.0
		for _, sample := range transitions {
			target := sample.reward
			if !sample.done {
				target += basePolicy.Gamma * maxActionValue(prevModels, sample.nextFeatures)
			}
			target = clampFloat(target, -25.0, 25.0)
			prediction := dot(prevModels[sample.action].Theta, sample.features)
			delta := target - prediction
			sumSquaredError += delta * delta
			arm := &models[sample.action]
			arm.A = outerAdd(arm.A, sample.features)
			addScaledInPlace(arm.B, sample.features, target)
			arm.Updates++
		}
		for idx := range models {
			models[idx].Theta = solveLinearSystem(models[idx].A, models[idx].B)
		}
		prevModels = copyLinearArmModels(models)
		trace = append(trace, fittedQTrainingSnapshot{
			Iteration:  iter + 1,
			BellmanMSE: sumSquaredError / maxFloat(float64(len(transitions)), 1),
			Policy: learnedFittedQPolicy{
				Actions:        copyLinearArmModels(prevModels),
				Gamma:          basePolicy.Gamma,
				Iterations:     basePolicy.Iterations,
				TrainingSeeds:  append([]int64(nil), basePolicy.TrainingSeeds...),
				HeldOutSeeds:   append([]int64(nil), basePolicy.HeldOutSeeds...),
				HeldOutRegimes: append([]string(nil), basePolicy.HeldOutRegimes...),
			},
		})
	}
	return trace
}

func initLinearArmModels(actions []learnedActionCandidate, featureDim int, ridge float64) []linearArmModel {
	models := make([]linearArmModel, 0, len(actions))
	for _, candidate := range actions {
		identity := identityMatrix(featureDim)
		for i := range identity {
			identity[i][i] = ridge
		}
		models = append(models, linearArmModel{
			Name:   candidate.Name,
			Action: candidate.Action,
			A:      identity,
			B:      make([]float64, featureDim),
			Theta:  make([]float64, featureDim),
		})
	}
	return models
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

func evaluateTinyMLPModel(cfg ScenarioConfig, model tinyMLPModel, seeds []int64) float64 {
	score := 0.0
	for _, seed := range seeds {
		trainingCfg := cfg
		trainingCfg.Seed = seed
		adapter := NewAdapter(trainingCfg)
		timestep := adapter.Reset()
		for !timestep.Done {
			action := chooseTinyMLPAction(adapter.ActionSpec(), timestep.Observation, model)
			timestep = adapter.Step(action)
			score += timestep.Reward
		}
	}
	return score / float64(len(seeds))
}

func initTinyMLPModel(actions []learnedActionCandidate, inputDim, hiddenDim int, rng *rand.Rand) tinyMLPModel {
	w1 := make([][]float64, hiddenDim)
	for h := 0; h < hiddenDim; h++ {
		w1[h] = make([]float64, inputDim)
		for i := 0; i < inputDim; i++ {
			w1[h][i] = rng.NormFloat64() * 0.08
		}
	}
	b1 := make([]float64, hiddenDim)
	outputDim := len(actions)
	w2 := make([][]float64, outputDim)
	for out := 0; out < outputDim; out++ {
		w2[out] = make([]float64, hiddenDim)
		for h := 0; h < hiddenDim; h++ {
			w2[out][h] = rng.NormFloat64() * 0.08
		}
	}
	b2 := make([]float64, outputDim)
	return tinyMLPModel{
		Actions:   append([]learnedActionCandidate(nil), actions...),
		InputDim:  inputDim,
		HiddenDim: hiddenDim,
		W1:        w1,
		B1:        b1,
		W2:        w2,
		B2:        b2,
	}
}

func collectBurstAwareDataset(cfg ScenarioConfig, actions []learnedActionCandidate, seeds []int64) ([][]float64, []int) {
	features := make([][]float64, 0, len(seeds)*cfg.TotalSteps)
	labels := make([]int, 0, len(seeds)*cfg.TotalSteps)
	for _, seed := range seeds {
		trainingCfg := cfg
		trainingCfg.Seed = seed
		adapter := NewAdapter(trainingCfg)
		timestep := adapter.Reset()
		spec := adapter.ActionSpec()
		for !timestep.Done {
			target := burstAwareAction(spec, timestep.Observation)
			features = append(features, observationFeatures(timestep.Observation))
			labels = append(labels, nearestCandidateIndex(spec, target, actions))
			timestep = adapter.Step(target)
		}
	}
	return features, labels
}

func collectOfflineTrajectory(cfg ScenarioConfig, seed int64, behavior PolicyController, actions []learnedActionCandidate, linucb *learnedLinUCBPolicy, tiny *tinyMLPModel, rng *rand.Rand, gamma float64) []offlinePolicySample {
	trainingCfg := cfg
	trainingCfg.Seed = seed
	adapter := NewAdapter(trainingCfg)
	spec := adapter.ActionSpec()
	timestep := adapter.Reset()
	trajectory := make([]policyStepSample, 0, trainingCfg.TotalSteps)
	for !timestep.Done {
		var action ControlAction
		switch behavior {
		case PolicyBurstAware:
			action = burstAwareAction(spec, timestep.Observation)
		case PolicyLearnedLinUCB:
			action = chooseLinUCBAction(spec, timestep.Observation, *linucb)
		case PolicyLearnedTinyMLP:
			action = chooseTinyMLPAction(spec, timestep.Observation, *tiny)
		default:
			action = actions[rng.Intn(len(actions))].Action
		}
		actionIdx := nearestCandidateIndex(spec, action, actions)
		features := observationFeatures(timestep.Observation)
		timestep = adapter.Step(action)
		trajectory = append(trajectory, policyStepSample{
			features: features,
			action:   actionIdx,
			reward:   timestep.Reward,
		})
	}
	return offlineSamplesFromTrajectory(trajectory, gamma)
}

func collectOfflineRandomTrajectory(cfg ScenarioConfig, seed int64, actions []learnedActionCandidate, rng *rand.Rand, gamma float64) []offlinePolicySample {
	trainingCfg := cfg
	trainingCfg.Seed = seed
	adapter := NewAdapter(trainingCfg)
	timestep := adapter.Reset()
	trajectory := make([]policyStepSample, 0, trainingCfg.TotalSteps)
	for !timestep.Done {
		actionIdx := rng.Intn(len(actions))
		features := observationFeatures(timestep.Observation)
		timestep = adapter.Step(actions[actionIdx].Action)
		trajectory = append(trajectory, policyStepSample{
			features: features,
			action:   actionIdx,
			reward:   timestep.Reward,
		})
	}
	return offlineSamplesFromTrajectory(trajectory, gamma)
}

func collectOfflineTransitionTrajectory(cfg ScenarioConfig, seed int64, behavior PolicyController, actions []learnedActionCandidate, linucb *learnedLinUCBPolicy, tiny *tinyMLPModel, offline *learnedOfflineContextualPolicy, rng *rand.Rand) []offlineTransitionSample {
	trainingCfg := cfg
	trainingCfg.Seed = seed
	adapter := NewAdapter(trainingCfg)
	spec := adapter.ActionSpec()
	timestep := adapter.Reset()
	trajectory := make([]offlineTransitionSample, 0, trainingCfg.TotalSteps)
	for !timestep.Done {
		var action ControlAction
		switch behavior {
		case PolicyBurstAware:
			action = burstAwareAction(spec, timestep.Observation)
		case PolicyLearnedLinUCB:
			action = chooseLinUCBAction(spec, timestep.Observation, *linucb)
		case PolicyLearnedTinyMLP:
			action = chooseTinyMLPAction(spec, timestep.Observation, *tiny)
		case PolicyLearnedOfflineContextual:
			action = chooseOfflineContextualAction(spec, timestep.Observation, *offline)
		default:
			action = actions[rng.Intn(len(actions))].Action
		}
		actionIdx := nearestCandidateIndex(spec, action, actions)
		features := observationFeatures(timestep.Observation)
		next := adapter.Step(action)
		trajectory = append(trajectory, offlineTransitionSample{
			features:     append([]float64(nil), features...),
			action:       actionIdx,
			reward:       next.Reward,
			nextFeatures: append([]float64(nil), observationFeatures(next.Observation)...),
			done:         next.Done,
		})
		timestep = next
	}
	return trajectory
}

func collectOfflineRandomTransitionTrajectory(cfg ScenarioConfig, seed int64, actions []learnedActionCandidate, rng *rand.Rand) []offlineTransitionSample {
	trainingCfg := cfg
	trainingCfg.Seed = seed
	adapter := NewAdapter(trainingCfg)
	timestep := adapter.Reset()
	trajectory := make([]offlineTransitionSample, 0, trainingCfg.TotalSteps)
	for !timestep.Done {
		actionIdx := rng.Intn(len(actions))
		features := observationFeatures(timestep.Observation)
		next := adapter.Step(actions[actionIdx].Action)
		trajectory = append(trajectory, offlineTransitionSample{
			features:     append([]float64(nil), features...),
			action:       actionIdx,
			reward:       next.Reward,
			nextFeatures: append([]float64(nil), observationFeatures(next.Observation)...),
			done:         next.Done,
		})
		timestep = next
	}
	return trajectory
}

func offlineSamplesFromTrajectory(trajectory []policyStepSample, gamma float64) []offlinePolicySample {
	returns := discountedReturns(trajectory, gamma)
	samples := make([]offlinePolicySample, 0, len(trajectory))
	for idx, sample := range trajectory {
		samples = append(samples, offlinePolicySample{
			features: append([]float64(nil), sample.features...),
			action:   sample.action,
			target:   returns[idx],
		})
	}
	return samples
}

func updateOfflineContextualPolicy(policy *learnedOfflineContextualPolicy, samples []offlinePolicySample) {
	for _, sample := range samples {
		if sample.action < 0 || sample.action >= len(policy.Actions) {
			continue
		}
		arm := &policy.Actions[sample.action]
		arm.A = outerAdd(arm.A, sample.features)
		addScaledInPlace(arm.B, sample.features, sample.target)
		arm.Theta = solveLinearSystem(arm.A, arm.B)
		arm.Updates++
	}
}

func trainTinyMLPSupervised(model *tinyMLPModel, features [][]float64, labels []int, epochs int, lr, l2 float64) {
	for epoch := 0; epoch < epochs; epoch++ {
		for idx, feature := range features {
			hidden, logits, probs := forwardTinyMLP(*model, feature)
			_ = logits
			target := labels[idx]
			dlogits := make([]float64, len(probs))
			for out := range probs {
				dlogits[out] = -probs[out]
			}
			dlogits[target] += 1.0
			applyTinyMLPGradients(model, feature, hidden, dlogits, lr, l2)
		}
	}
}

func trainTinyMLPPolicyGradient(cfg ScenarioConfig, model *tinyMLPModel, seeds []int64, rng *rand.Rand, episodes int, lr, gamma, l2 float64) {
	for episode := 0; episode < episodes; episode++ {
		trainingCfg := cfg
		trainingCfg.Seed = seeds[episode%len(seeds)]
		adapter := NewAdapter(trainingCfg)
		timestep := adapter.Reset()
		trajectory := make([]policyStepSample, 0, trainingCfg.TotalSteps)
		for !timestep.Done {
			features := observationFeatures(timestep.Observation)
			hidden, _, probs := forwardTinyMLP(*model, features)
			actionIdx := sampleCategorical(probs, rng)
			timestep = adapter.Step(model.Actions[actionIdx].Action)
			trajectory = append(trajectory, policyStepSample{
				features: append([]float64(nil), features...),
				hidden:   append([]float64(nil), hidden...),
				probs:    append([]float64(nil), probs...),
				action:   actionIdx,
				reward:   timestep.Reward,
			})
		}
		returns := discountedReturns(trajectory, gamma)
		mean, std := meanStd(returns)
		for idx, sample := range trajectory {
			advantage := returns[idx] - mean
			if std > 1e-9 {
				advantage /= std
			}
			if advantage > 4 {
				advantage = 4
			}
			if advantage < -4 {
				advantage = -4
			}
			dlogits := make([]float64, len(sample.probs))
			for out := range sample.probs {
				dlogits[out] = -sample.probs[out] * advantage
			}
			dlogits[sample.action] += advantage
			applyTinyMLPGradients(model, sample.features, sample.hidden, dlogits, lr, l2)
		}
	}
}

func discountedReturns(trajectory []policyStepSample, gamma float64) []float64 {
	returns := make([]float64, len(trajectory))
	running := 0.0
	for idx := len(trajectory) - 1; idx >= 0; idx-- {
		running = trajectory[idx].reward + gamma*running
		returns[idx] = running
	}
	return returns
}

func nearestCandidateIndex(spec ActionSpec, target ControlAction, actions []learnedActionCandidate) int {
	bestIdx := 0
	bestDistance := math.Inf(1)
	for idx, candidate := range actions {
		distance := actionDistance(spec, target, candidate.Action)
		if distance < bestDistance {
			bestDistance = distance
			bestIdx = idx
		}
	}
	return bestIdx
}

func actionDistance(spec ActionSpec, left, right ControlAction) float64 {
	distance := 0.0
	if spec.SupportsBatchWindowControl {
		distance += math.Pow(float64(readIntAction(left.TargetBatchWindowSteps, spec.MinBatchWindowSteps)-readIntAction(right.TargetBatchWindowSteps, spec.MinBatchWindowSteps))/float64(maxInt(1, spec.MaxBatchWindowSteps)), 2)
	}
	if spec.SupportsRiskLimitScale {
		distance += math.Pow(readFloatAction(left.RiskLimitScale, 1.0)-readFloatAction(right.RiskLimitScale, 1.0), 2)
	}
	if spec.SupportsTieBreakToggle && readBoolAction(left.RandomizeTieBreak) != readBoolAction(right.RandomizeTieBreak) {
		distance += 1.0
	}
	if spec.SupportsReleaseCadence {
		distance += math.Pow(float64(readIntAction(left.ReleaseCadenceSteps, 0)-readIntAction(right.ReleaseCadenceSteps, 0))/float64(maxInt(1, spec.MaxReleaseCadenceSteps)), 2)
	}
	if spec.SupportsPriceAggressionBias {
		distance += math.Pow(float64(readInt64Action(left.PriceAggressionBias, 0)-readInt64Action(right.PriceAggressionBias, 0))/float64(maxInt64(1, absInt64(spec.MaxPriceAggressionBias))), 2)
	}
	return distance
}

func readIntAction(value *int, fallback int) int {
	if value == nil {
		return fallback
	}
	return *value
}

func readFloatAction(value *float64, fallback float64) float64 {
	if value == nil {
		return fallback
	}
	return *value
}

func readBoolAction(value *bool) bool {
	if value == nil {
		return false
	}
	return *value
}

func readInt64Action(value *int64, fallback int64) int64 {
	if value == nil {
		return fallback
	}
	return *value
}

func forwardTinyMLP(model tinyMLPModel, features []float64) ([]float64, []float64, []float64) {
	hidden := make([]float64, model.HiddenDim)
	for h := 0; h < model.HiddenDim; h++ {
		sum := model.B1[h]
		for i := 0; i < model.InputDim; i++ {
			sum += model.W1[h][i] * features[i]
		}
		hidden[h] = math.Tanh(sum)
	}
	logits := make([]float64, len(model.Actions))
	for out := range model.Actions {
		score := model.B2[out]
		for h := 0; h < model.HiddenDim; h++ {
			score += model.W2[out][h] * hidden[h]
		}
		logits[out] = score
	}
	return hidden, logits, softmax(logits)
}

func applyTinyMLPGradients(model *tinyMLPModel, features, hidden, dlogits []float64, lr, l2 float64) {
	hiddenGrad := make([]float64, model.HiddenDim)
	for h := 0; h < model.HiddenDim; h++ {
		acc := 0.0
		for out := range model.Actions {
			acc += model.W2[out][h] * dlogits[out]
		}
		hiddenGrad[h] = (1 - hidden[h]*hidden[h]) * acc
	}
	for out := range model.Actions {
		for h := 0; h < model.HiddenDim; h++ {
			model.W2[out][h] = model.W2[out][h]*(1-lr*l2) + lr*dlogits[out]*hidden[h]
		}
		model.B2[out] += lr * dlogits[out]
	}
	for h := 0; h < model.HiddenDim; h++ {
		for i := 0; i < model.InputDim; i++ {
			model.W1[h][i] = model.W1[h][i]*(1-lr*l2) + lr*hiddenGrad[h]*features[i]
		}
		model.B1[h] += lr * hiddenGrad[h]
	}
}

func softmax(logits []float64) []float64 {
	maxLogit := logits[0]
	for _, value := range logits[1:] {
		if value > maxLogit {
			maxLogit = value
		}
	}
	probs := make([]float64, len(logits))
	sum := 0.0
	for idx, value := range logits {
		probs[idx] = math.Exp(value - maxLogit)
		sum += probs[idx]
	}
	if sum <= 0 {
		for idx := range probs {
			probs[idx] = 1.0 / float64(len(probs))
		}
		return probs
	}
	for idx := range probs {
		probs[idx] /= sum
	}
	return probs
}

func sampleCategorical(probs []float64, rng *rand.Rand) int {
	target := rng.Float64()
	cumulative := 0.0
	for idx, prob := range probs {
		cumulative += prob
		if target <= cumulative {
			return idx
		}
	}
	return len(probs) - 1
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

func chooseOfflineContextualAction(spec ActionSpec, observation Observation, policy learnedOfflineContextualPolicy) ControlAction {
	if len(policy.Actions) == 0 {
		return fallbackBanditAction(spec)
	}
	features := observationFeatures(observation)
	bestIdx := 0
	bestScore := math.Inf(-1)
	for idx, arm := range policy.Actions {
		score := dot(arm.Theta, features)
		if arm.Updates == 0 {
			score = math.Inf(-1)
		}
		if score > bestScore {
			bestScore = score
			bestIdx = idx
		}
	}
	if bestScore == math.Inf(-1) {
		return fallbackBanditAction(spec)
	}
	return policy.Actions[bestIdx].Action
}

func chooseFittedQAction(spec ActionSpec, observation Observation, policy learnedFittedQPolicy) ControlAction {
	if len(policy.Actions) == 0 {
		return fallbackBanditAction(spec)
	}
	features := observationFeatures(observation)
	bestIdx := 0
	bestScore := math.Inf(-1)
	for idx, arm := range policy.Actions {
		score := dot(arm.Theta, features)
		if arm.Updates == 0 {
			score = math.Inf(-1)
		}
		if score > bestScore {
			bestScore = score
			bestIdx = idx
		}
	}
	if bestScore == math.Inf(-1) {
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

func chooseTinyMLPAction(spec ActionSpec, observation Observation, model tinyMLPModel) ControlAction {
	_, logits, _ := forwardTinyMLP(model, observationFeatures(observation))
	bestIdx := 0
	bestScore := math.Inf(-1)
	for out, score := range logits {
		if score > bestScore {
			bestScore = score
			bestIdx = out
		}
	}
	if bestIdx < 0 || bestIdx >= len(model.Actions) {
		return fallbackBanditAction(spec)
	}
	return model.Actions[bestIdx].Action
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

func maxInt64(a, b int64) int64 {
	if a > b {
		return a
	}
	return b
}

func absInt64(v int64) int64 {
	if v < 0 {
		return -v
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

func copyLinearArmModels(src []linearArmModel) []linearArmModel {
	dst := make([]linearArmModel, len(src))
	for i := range src {
		dst[i] = linearArmModel{
			Name:    src[i].Name,
			Action:  src[i].Action,
			A:       copyMatrix(src[i].A),
			B:       append([]float64(nil), src[i].B...),
			Theta:   append([]float64(nil), src[i].Theta...),
			Updates: src[i].Updates,
		}
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

func clampFloat(v, minV, maxV float64) float64 {
	if v < minV {
		return minV
	}
	if v > maxV {
		return maxV
	}
	return v
}

func maxActionValue(models []linearArmModel, features []float64) float64 {
	best := math.Inf(-1)
	for _, arm := range models {
		if arm.Updates == 0 {
			continue
		}
		score := dot(arm.Theta, features)
		if score > best {
			best = score
		}
	}
	if best == math.Inf(-1) {
		return 0
	}
	return best
}

func rankedBanditModels(policy learnedLinUCBPolicy) []linearArmModel {
	models := append([]linearArmModel(nil), policy.Actions...)
	sort.Slice(models, func(i, j int) bool {
		return models[i].Updates > models[j].Updates
	})
	return models
}

func trainingRandomSeed(cfg ScenarioConfig) int64 {
	return int64(len(cfg.Agents))*97 + int64(cfg.TotalSteps)*31 + int64(cfg.AdaptiveMinWindowSteps+cfg.AdaptiveMaxWindowSteps) + 20260307
}

func policyCacheKey(cfg ScenarioConfig, policy PolicyController) string {
	keyCfg := cfg
	keyCfg.Seed = 0
	keyCfg.Name = ""
	keyCfg.PolicyController = policy
	raw, err := json.Marshal(keyCfg)
	if err != nil {
		return string(policy)
	}
	return string(raw)
}

func (a *Adapter) computeReward(delta MetricsDelta) float64 {
	return a.rewardWeights.FillWeight*float64(delta.FillsDelta) -
		a.rewardWeights.SpreadPenalty*delta.SpreadDelta -
		a.rewardWeights.PriceImpactPenalty*delta.PriceImpactDelta -
		a.rewardWeights.QueuePenalty*absFloat(delta.QueuePriorityDelta) -
		a.rewardWeights.ArbitragePenalty*delta.ArbitrageProfitDelta -
		a.rewardWeights.AdversePenalty*delta.RetailAdverseDelta -
		a.rewardWeights.WelfarePenalty*delta.WelfareDispersionDelta -
		a.rewardWeights.SurplusGapPenalty*maxFloat(delta.SurplusTransferGapDelta, 0) +
		a.rewardWeights.RetailSurplusWeight*delta.RetailSurplusDelta -
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
		RetailSurplusDelta:        next.RetailSurplusPerUnit - prev.RetailSurplusPerUnit,
		RetailAdverseDelta:        next.RetailAdverseSelectionRate - prev.RetailAdverseSelectionRate,
		WelfareDispersionDelta:    next.WelfareDispersion - prev.WelfareDispersion,
		SurplusTransferGapDelta:   next.SurplusTransferGap - prev.SurplusTransferGap,
		RiskRejectionsDelta:       next.RiskRejections - prev.RiskRejections,
		ConservationBreachesDelta: next.ConservationBreaches - prev.ConservationBreaches,
	}
}
