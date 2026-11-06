# KubernetesExposureGeneratedCertificateUnion

Certificate publication or reference mode for Kubernetes public endpoints.


## Supported Types

### `models.KubernetesExposureGeneratedCertificateTLSSecretRef`

```typescript
const value: models.KubernetesExposureGeneratedCertificateTLSSecretRef = {
  secretName: "<value>",
  mode: "tlsSecretRef",
};
```

### `models.KubernetesExposureGeneratedCertificateManagedAcmImport`

```typescript
const value: models.KubernetesExposureGeneratedCertificateManagedAcmImport = {
  mode: "managedAcmImport",
};
```

### `models.KubernetesExposureGeneratedCertificateAwsAcmArn`

```typescript
const value: models.KubernetesExposureGeneratedCertificateAwsAcmArn = {
  certificateArn: "<value>",
  mode: "awsAcmArn",
};
```

### `models.KubernetesExposureGeneratedCertificateManagedTLSSecret`

```typescript
const value: models.KubernetesExposureGeneratedCertificateManagedTLSSecret = {
  mode: "managedTlsSecret",
  secretNameTemplate: "<value>",
};
```

### `models.KubernetesExposureGeneratedCertificateNone`

```typescript
const value: models.KubernetesExposureGeneratedCertificateNone = {
  mode: "none",
};
```
