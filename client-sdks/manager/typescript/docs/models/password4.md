# Password4

Password literal or a `secretKeyRef` to a Kubernetes Secret. Kubernetes workloads get the
password from the Secret as an environment variable, which the SDK inserts after it
parses the binding, so any character is safe.


## Supported Types

### `models.Password3`

```typescript
const value: models.Password3 = "<value>";
```
