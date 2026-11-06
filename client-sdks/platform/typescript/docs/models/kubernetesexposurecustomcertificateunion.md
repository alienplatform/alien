# KubernetesExposureCustomCertificateUnion

Certificate publication or reference mode for Kubernetes public endpoints.


## Supported Types

### `models.KubernetesExposureCustomCertificateTLSSecretRef`

```typescript
const value: models.KubernetesExposureCustomCertificateTLSSecretRef = {
  secretName: "<value>",
  mode: "tlsSecretRef",
};
```

### `models.KubernetesExposureCustomCertificateManagedAcmImport`

```typescript
const value: models.KubernetesExposureCustomCertificateManagedAcmImport = {
  mode: "managedAcmImport",
};
```

### `models.KubernetesExposureCustomCertificateAwsAcmArn`

```typescript
const value: models.KubernetesExposureCustomCertificateAwsAcmArn = {
  certificateArn: "<value>",
  mode: "awsAcmArn",
};
```

### `models.KubernetesExposureCustomCertificateManagedTLSSecret`

```typescript
const value: models.KubernetesExposureCustomCertificateManagedTLSSecret = {
  mode: "managedTlsSecret",
  secretNameTemplate: "<value>",
};
```

### `models.KubernetesExposureCustomCertificateNone`

```typescript
const value: models.KubernetesExposureCustomCertificateNone = {
  mode: "none",
};
```

