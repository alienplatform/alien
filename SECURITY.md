# Security Policy

We take security seriously at Alien. We appreciate your efforts to responsibly disclose vulnerabilities and will make every effort to acknowledge your contributions.

## Reporting a Vulnerability

**Please do not report security vulnerabilities through public GitHub issues.**

Instead, please report vulnerabilities via [GitHub Security Advisories](https://github.com/alienplatform/alien/security/advisories/new) or send security-related reports to **[security@alien.dev](mailto:security@alien.dev)**.

### What to Include

When reporting a vulnerability, please include:

1. **Description** — A clear description of the vulnerability
2. **Impact** — What an attacker could achieve by exploiting this issue
3. **Steps to reproduce** — Detailed steps to reproduce the vulnerability
4. **Affected components** — Which component is affected (CLI, Manager, Agent, SDK, Runtime, Terraform modules, etc.)
5. **Affected versions** — Which versions are affected, if known
6. **Suggested fix** — If you have ideas on how to fix it (optional)

### What to Expect

- **Acknowledgment** — We will acknowledge receipt of your report within 48 hours
- **Updates** — We will keep you informed of our progress as we investigate
- **Resolution** — We aim to resolve critical vulnerabilities within 90 days

## Confidentiality

All reports will be kept confidential. We will not share your information with third parties without your consent, except as required by law.

## Supported Versions

We recommend always running the latest version of Alien. Security fixes are only applied to the latest release.

## Scope

This security policy applies to:

- All code in the [alienplatform/alien](https://github.com/alienplatform/alien) GitHub repo
- Alien-related services at alien.dev

### Out of Scope

The following are generally not considered security vulnerabilities:

- Issues in third-party dependencies (please report these upstream)
- Social engineering attacks
- Denial of service attacks that require authenticated access
- Issues requiring physical access to a user's device
- Security misconfigurations in user-managed infrastructure outside of Alien's defaults

---

## Security Advisories

Security advisories are issued when a confirmed vulnerability can be exploited by a remote or non-local actor. The following are generally treated as **bug reports rather than security advisories**:

- Resource exhaustion that requires local access to trigger
- Issues in `alien dev` that cannot be exploited without direct access to the developer's machine

Use [GitHub Issues](https://github.com/alienplatform/alien/issues) to report bugs.

---

Thank you for helping keep Alien and our community safe!
