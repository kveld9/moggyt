# Security Policy

## Supported Versions

We release patches and security fixes for currently maintained versions.
The table below outlines the support status for recent releases:

| Version | Supported          |
| ------- | ------------------ |
| 1.x.x   | :white_check_mark: |
| < 1.0.0 | :x:                |

## Reporting a Vulnerability

We take the security of this project seriously. If you discover a security vulnerability, please follow responsible
disclosure guidelines. **Do NOT report security vulnerabilities via public GitHub issues, discussions, or pull requests.**

### How to Report

1. **GitHub Private Vulnerability Reporting (Preferred):**
   If enabled on this repository, navigate to the **Security** tab, click **Advisories**, and select
   **Report a vulnerability**. This creates a confidential communication channel directly with maintainers.

2. **Email Disclosure:**
   If private reporting is not accessible, send an email to
   `179108235+kveld9@users.noreply.github.com` (or project maintainers) with the subject:
   `[SECURITY] Vulnerability Report: <Brief Description>`.

### Information to Include

Please provide as much information as possible to help us reproduce and resolve the issue quickly:

- Type of issue (e.g., buffer overflow, SQL injection, unauthorized access, cross-site scripting).
- Full paths of source file(s) related to the manifestation of the issue.
- The location of the affected source code (tag/branch/commit or direct URL).
- Any special configuration required to reproduce the issue.
- Step-by-step instructions to reproduce the issue (proof-of-concept script or sample inputs).
- Impact assessment: what an attacker could achieve by exploiting this issue.

### Response Timeline

- **Initial Acknowledgment:** Within 48 hours of receipt.
- **Triage & Assessment:** Within 5 business days, confirming whether the issue is accepted as a vulnerability.
- **Remediation & Patch:** A fix will be developed in a private branch/advisory.
- **Public Disclosure:** Coordinated disclosure after a patch is released and users have had reasonable time to upgrade.

Thank you for helping keep this project and its users secure.
