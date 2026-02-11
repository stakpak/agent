# Security Review

Perform a comprehensive security review of the current codebase:

1. **Secrets & Credentials**
   - Check for hardcoded secrets, API keys, passwords
   - Verify secrets are loaded from environment variables or secret managers

2. **Authentication & Authorization**
   - Review authentication logic for vulnerabilities
   - Check authorization controls and access patterns
   - Look for privilege escalation risks

3. **Input Validation**
   - Identify SQL injection vulnerabilities
   - Check for XSS (Cross-Site Scripting) risks
   - Verify input sanitization and validation

4. **Dependencies**
   - Check for known vulnerable dependencies
   - Review dependency versions for security patches

5. **Data Protection**
   - Review sensitive data handling
   - Check encryption usage for data at rest and in transit

Focus on high-severity issues first. Provide specific file locations and remediation steps.
