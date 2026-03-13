import type { AIReview, PullRequest } from "./schemas.js"

/**
 * Generate AI-powered code review for a pull request.
 * In production, this would call an LLM API with the PR diff and context.
 * For demo purposes, we generate realistic review data based on PR characteristics.
 */
export function generateAIReview(pr: PullRequest): AIReview {
  const reviews: Record<number, AIReview> = {
    101: {
      prNumber: 101,
      summary:
        "Strong authentication implementation with proper session handling. Minor improvements suggested for error handling.",
      overallRating: "good",
      issues: [
        {
          severity: "medium",
          category: "security",
          title: "Session token not invalidated on logout",
          description:
            "The logout handler clears the session cookie but doesn't invalidate the token in Redis, potentially allowing session replay attacks.",
          file: "src/auth/session.ts",
          line: 45,
          suggestion: "Add `await redis.del(`session:${token}`)` before clearing the cookie",
        },
        {
          severity: "low",
          category: "best-practice",
          title: "Hard-coded session timeout",
          description:
            "Session timeout is hard-coded to 24 hours. Consider making this configurable via environment variable.",
          file: "src/auth/session.ts",
          line: 23,
          suggestion: "Use `parseInt(process.env.SESSION_TIMEOUT || '86400')` instead",
        },
      ],
      highlights: [
        "Proper input validation on login form",
        "CSRF protection implemented correctly",
        "Rate limiting applied to authentication endpoints",
      ],
      codeExamples: [
        {
          file: "src/auth/login.ts",
          lineStart: 34,
          lineEnd: 42,
          language: "typescript",
          code: `async function validateCredentials(email: string, password: string) {
  const user = await db.users.findByEmail(email)
  if (!user) {
    throw new AuthError('Invalid credentials')
  }
  
  const valid = await bcrypt.compare(password, user.passwordHash)
  if (!valid) {
    throw new AuthError('Invalid credentials')
  }`,
        },
      ],
      rawAnalysis: `## Security Analysis

I've reviewed this authentication implementation and found it to be generally well-structured. The code follows OWASP best practices for authentication flows.

### Key Observations:

1. **Password Hashing**: Using bcrypt with appropriate cost factor (12 rounds)
2. **Session Management**: Redis-backed sessions with secure cookies (httpOnly, sameSite)
3. **Input Validation**: All user inputs are validated using zod schemas
4. **Rate Limiting**: Applied at middleware level (5 attempts per 15 minutes)

### Concerns:

The main security concern is the session invalidation logic. While the cookie is cleared on logout, the session token remains in Redis until TTL expiration. An attacker who captures a session token could potentially use it after the user logs out.

### Recommendation:

Implement proper session invalidation:

\`\`\`typescript
export async function logout(token: string) {
  await redis.del(\`session:\${token}\`)  // Add this line
  clearSessionCookie()
}
\`\`\`

This ensures defense-in-depth - even if the cookie is somehow preserved or replayed, the server-side session is invalidated.`,
      reviewedAt: "2024-01-01T13:30:00Z",
    },
    102: {
      prNumber: 102,
      summary:
        "Complex billing refactor with good test coverage, but performance concerns in high-volume scenarios.",
      overallRating: "needs-work",
      issues: [
        {
          severity: "high",
          category: "performance",
          title: "N+1 query in billing calculation",
          description:
            "The ledger calculation loops through transactions and makes individual database queries for each account balance check.",
          file: "src/billing/ledger.ts",
          line: 67,
          suggestion: "Batch load all account balances in a single query before the loop",
        },
        {
          severity: "medium",
          category: "bug-risk",
          title: "Race condition in concurrent billing updates",
          description:
            "Multiple concurrent billing updates could cause race conditions when updating account balances without proper locking.",
          file: "src/billing/ledger.ts",
          line: 89,
          suggestion: "Use database transactions with SELECT FOR UPDATE or optimistic locking",
        },
        {
          severity: "low",
          category: "maintainability",
          title: "Complex nested conditionals",
          description:
            "The pricing logic has deeply nested if/else blocks that are hard to follow and test.",
          file: "src/billing/pricing.ts",
          line: 145,
          suggestion: "Refactor into a pricing strategy pattern or use a pricing table",
        },
      ],
      highlights: [
        "Comprehensive unit test coverage (95%)",
        "Detailed audit logging for all billing events",
        "Proper decimal arithmetic for currency calculations",
      ],
      codeExamples: [
        {
          file: "src/billing/ledger.ts",
          lineStart: 65,
          lineEnd: 75,
          language: "typescript",
          code: `for (const transaction of transactions) {
  // ⚠️ N+1 query problem - this makes a DB call on each iteration
  const balance = await getAccountBalance(transaction.accountId)
  
  if (balance < transaction.amount) {
    throw new InsufficientFundsError(transaction.accountId)
  }
  
  await debitAccount(transaction.accountId, transaction.amount)
  total += transaction.amount
}`,
        },
      ],
      rawAnalysis: `## Performance & Scalability Review

This billing pipeline refactor introduces significant technical debt that will impact performance at scale.

### Critical Issue: N+1 Query Pattern

The most concerning issue is in \`ledger.ts\` lines 65-75. The code iterates through transactions and makes individual database queries for each account balance check. In a high-volume scenario with 1000 transactions, this becomes 1000 separate database queries.

**Impact**: At 10ms per query, this adds 10 seconds of latency for a single billing batch. This doesn't scale.

**Fix**:

\`\`\`typescript
// Batch load all balances upfront
const accountIds = transactions.map(t => t.accountId)
const balances = await getAccountBalances(accountIds) // Single query
const balanceMap = new Map(balances.map(b => [b.accountId, b.balance]))

for (const transaction of transactions) {
  const balance = balanceMap.get(transaction.accountId)
  // ... rest of logic
}
\`\`\`

### Race Condition Risk

The concurrent update logic lacks proper synchronization. Two simultaneous billing runs could read the same balance, both pass validation, and double-debit an account.

**Solution**: Use database-level locking or implement optimistic concurrency control.

### Test Coverage

On the positive side, the test coverage is excellent at 95%. The audit logging is comprehensive and will be valuable for debugging production issues.`,
      reviewedAt: "2024-01-03T12:00:00Z",
    },
    103: {
      prNumber: 103,
      summary:
        "Well-architected async worker system. Excellent use of queues and error handling patterns.",
      overallRating: "excellent",
      issues: [
        {
          severity: "info",
          category: "maintainability",
          title: "Consider adding worker metrics",
          description:
            "The worker system would benefit from Prometheus-style metrics for monitoring queue depth, processing time, and error rates.",
          suggestion: "Add instrumentation using a metrics library like prom-client",
        },
      ],
      highlights: [
        "Robust error handling with exponential backoff retry",
        "Dead letter queue for failed messages",
        "Graceful shutdown handling with in-flight job completion",
        "Worker pool with dynamic scaling based on queue depth",
        "Comprehensive logging with structured context",
      ],
      codeExamples: [
        {
          file: "src/workers/ingest.ts",
          lineStart: 89,
          lineEnd: 105,
          language: "typescript",
          code: `async function processJob(job: Job): Promise<void> {
  const logger = createLogger({ jobId: job.id, type: job.type })
  
  try {
    logger.info('Processing job', { attempts: job.attempts })
    
    await job.process()
    await job.markComplete()
    
    logger.info('Job completed successfully')
  } catch (error) {
    logger.error('Job failed', { error })
    
    if (job.attempts >= MAX_RETRIES) {
      await moveToDeadLetterQueue(job)
      logger.warn('Job moved to DLQ after max retries')
    } else {
      await scheduleRetry(job, calculateBackoff(job.attempts))
    }
  }
}`,
        },
      ],
      rawAnalysis: `## Architecture Review: Async Ingestion Workers

This is an exemplary implementation of a production-grade worker system. The code demonstrates deep understanding of distributed systems patterns.

### Architectural Highlights:

**1. Resilience**
- Exponential backoff retry with jitter prevents thundering herd
- Dead letter queue ensures failed messages aren't lost
- Circuit breaker pattern protects downstream services

**2. Observability**
- Structured logging with contextual fields (jobId, traceId)
- All state transitions are logged
- Clear error messages with actionable context

**3. Graceful Degradation**
- Shutdown handling: waits for in-flight jobs before terminating
- Worker pool dynamically scales based on queue depth
- Rate limiting prevents overwhelming downstream services

### Code Quality:

The separation of concerns is excellent:
- \`Queue.ts\` handles message broker interaction
- \`Worker.ts\` manages job processing lifecycle
- \`Ingest.ts\` contains business logic

Error handling follows the "let it crash" philosophy - errors propagate cleanly with rich context rather than being swallowed.

### Minor Suggestion:

The only improvement I'd suggest is adding metrics instrumentation. Key metrics to track:
- Queue depth over time
- Job processing duration (p50, p95, p99)
- Error rate by job type
- Worker pool size

This would enable proactive alerting before issues impact users.

### Verdict:

This code is production-ready. Ship it with confidence.`,
      reviewedAt: "2024-01-08T10:00:00Z",
    },
  }

  // Return mock review for known PRs, or generate a basic one
  return (
    reviews[pr.number] || {
      prNumber: pr.number,
      summary: "Standard code review completed. No critical issues found.",
      overallRating: "good" as const,
      issues: [],
      highlights: ["Code follows project conventions", "Tests are passing"],
      codeExamples: [],
      rawAnalysis:
        "## Standard Review\n\nThis pull request has been reviewed and appears to follow standard practices. No critical issues detected.",
      reviewedAt: new Date().toISOString(),
    }
  )
}
