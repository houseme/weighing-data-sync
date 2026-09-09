package main

import (
	"net/url"
	"testing"
	"time"
)

func TestSignatureCompatibleWithReceiver(t *testing.T) {
	got := signRequest("secret", "GET", "/weighing-data-sync/records", "include_raw=true&limit=100", "1787846400", "nonce", nil)
	const want = "de3f34071cd00a2425069798f1943dffe51176e8f234ef01b90638556df57215"
	if got != want {
		t.Fatalf("signature = %s", got)
	}
}

func TestCanonicalQuery(t *testing.T) {
	values := url.Values{"b": {"2", "1"}, "signature": {"skip"}, "a": {"x y"}}
	if got := canonicalQuery(values); got != "a=x+y&b=1&b=2" {
		t.Fatalf("query = %s", got)
	}
}

func TestRetryDelayCaps(t *testing.T) {
	if got := retryDelay(1); got != time.Second {
		t.Fatalf("first retry = %s", got)
	}
	if got := retryDelay(12); got != 128*time.Second {
		t.Fatalf("capped retry = %s", got)
	}
}
