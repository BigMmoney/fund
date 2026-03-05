package utils

import (
	"crypto/sha256"
	"encoding/hex"
	"fmt"
	"math/rand"
	"time"

	"github.com/google/uuid"
)

func init() {
	rand.Seed(time.Now().UnixNano())
}

// GenerateID generates a unique ID
func GenerateID() string {
	return uuid.New().String()
}

// GenerateUUID generates a UUID string
func GenerateUUID() string {
	return uuid.New().String()
}

// RandomInt generates a random integer in range [min, max)
func RandomInt(min, max int) int {
	if min >= max {
		return min
	}
	return min + rand.Intn(max-min)
}

// GenerateOpID generates an operation ID
func GenerateOpID(prefix string) string {
	timestamp := time.Now().UnixNano()
	return fmt.Sprintf("%s:%d:%s", prefix, timestamp, GenerateID()[:8])
}

// GenerateChainOpID generates an operation ID from chain event
func GenerateChainOpID(chainID, txHash string, logIndex int) string {
	return fmt.Sprintf("%s:%s:%d", chainID, txHash, logIndex)
}

// HashString generates a SHA256 hash of a string
func HashString(s string) string {
	hash := sha256.Sum256([]byte(s))
	return hex.EncodeToString(hash[:])
}

// MinInt64 returns the minimum of two int64 values
func MinInt64(a, b int64) int64 {
	if a < b {
		return a
	}
	return b
}

// MaxInt64 returns the maximum of two int64 values
func MaxInt64(a, b int64) int64 {
	if a > b {
		return a
	}
	return b
}

// AbsInt64 returns the absolute value of an int64
func AbsInt64(n int64) int64 {
	if n < 0 {
		return -n
	}
	return n
}

// FormatAccount formats an account identifier
func FormatAccount(userID, accountType string) string {
	return fmt.Sprintf("U:%s:%s", userID, accountType)
}

// FormatMarketAccount formats a market account identifier
func FormatMarketAccount(marketID, accountType string) string {
	return fmt.Sprintf("M:%s:%s", marketID, accountType)
}

// FormatOutcomeAccount formats an outcome account identifier
func FormatOutcomeAccount(userID, marketID string, outcome int) string {
	return fmt.Sprintf("U:%s:OUTCOME:%s:%d", userID, marketID, outcome)
}

// IsExpired checks if a time has passed
func IsExpired(expiresAt time.Time) bool {
	return time.Now().After(expiresAt)
}

// TimePtr returns a pointer to a time.Time
func TimePtr(t time.Time) *time.Time {
	return &t
}

// IntPtr returns a pointer to an int
func IntPtr(i int) *int {
	return &i
}
