# StackSettingsCertificateUnion2

Certificate publication or reference mode for Kubernetes public endpoints.

## Supported Types

### `models.StackSettingsCertificateTLSSecretRef2`

```typescript
const value: models.StackSettingsCertificateTLSSecretRef2 = {
  secretName: "<value>",
  mode: "tlsSecretRef",
};
```

### `models.StackSettingsCertificateManagedAcmImport2`

```typescript
const value: models.StackSettingsCertificateManagedAcmImport2 = {
  mode: "managedAcmImport",
};
```

### `models.StackSettingsCertificateAwsAcmArn2`

```typescript
const value: models.StackSettingsCertificateAwsAcmArn2 = {
  certificateArn: "<value>",
  mode: "awsAcmArn",
};
```

### `models.StackSettingsCertificateManagedTLSSecret2`

```typescript
const value: models.StackSettingsCertificateManagedTLSSecret2 = {
  mode: "managedTlsSecret",
  secretNameTemplate: "<value>",
};
```

### `models.StackSettingsCertificateNone2`

```typescript
const value: models.StackSettingsCertificateNone2 = {
  mode: "none",
};
```
