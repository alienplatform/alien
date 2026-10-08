# SyncListResponseCertificateUnion2

Certificate publication or reference mode for Kubernetes public endpoints.


## Supported Types

### `models.SyncListResponseCertificateTLSSecretRef2`

```typescript
const value: models.SyncListResponseCertificateTLSSecretRef2 = {
  secretName: "<value>",
  mode: "tlsSecretRef",
};
```

### `models.SyncListResponseCertificateManagedAcmImport2`

```typescript
const value: models.SyncListResponseCertificateManagedAcmImport2 = {
  mode: "managedAcmImport",
};
```

### `models.SyncListResponseCertificateAwsAcmArn2`

```typescript
const value: models.SyncListResponseCertificateAwsAcmArn2 = {
  certificateArn: "<value>",
  mode: "awsAcmArn",
};
```

### `models.SyncListResponseCertificateManagedTLSSecret2`

```typescript
const value: models.SyncListResponseCertificateManagedTLSSecret2 = {
  mode: "managedTlsSecret",
  secretNameTemplate: "<value>",
};
```

### `models.SyncListResponseCertificateNone2`

```typescript
const value: models.SyncListResponseCertificateNone2 = {
  mode: "none",
};
```

