# Code signing policy

## Status

RECON Windows binaries are currently **unsigned**. GitHub Actions builds are not Authenticode-signed and may be blocked by Windows Smart App Control. Checksums verify that a downloaded artifact has not changed relative to the published workflow output; they **do not** establish publisher identity or replace code signing.

We plan to apply to the [SignPath Foundation](https://signpath.org/) free open-source signing program after RECON has a maintained, documented release. Admission is subject to SignPath's approval. We do not claim affiliation, enrollment, or signing from SignPath until that actually occurs.

## Source and release process

- Canonical source: https://github.com/jzxx26/recon
- Committers, reviewers, and signing approvers: repository maintainer(s), currently [jzxx26](https://github.com/jzxx26). Maintainers must enable multi-factor authentication.
- Only binaries built by GitHub Actions from reviewed source and pinned release commits may eventually be submitted for signing.
- Pull requests must pass Windows build and tests before merging. Signed releases will require separate maintainer approval and verification of signature and timestamp.
- Do not sign unreviewed pull-request artifacts or user-provided binaries.

## Data and privacy

RECON stores match records in local SQLite and saves the configured HenrikDev API key in the operating system's credential store. The user explicitly initiates HenrikDev network requests by pressing Sync. Future optional cloud sync and clip uploads will require explicit consent and an updated policy.

## Verify an eventual signed release

On Windows PowerShell:

```powershell
Get-AuthenticodeSignature .\RECON.exe | Format-List Status,SignerCertificate,TimeStamperCertificate
Get-FileHash .\RECON.exe -Algorithm SHA256
```

Do not treat a `NotSigned` result as signed, and do not install untrusted certificates merely to suppress Windows warnings.

Once SignPath approves the project, we can integrate their official signing action, artifact submission, and signed release validation into the trusted release workflow. At that point the policy will identify SignPath's certificate and formal team roles.