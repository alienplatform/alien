# ManagerRetryResponseNetworkUnion3


## Supported Types

### `models.ManagerRetryResponseNetworkUseDefault3`

```typescript
const value: models.ManagerRetryResponseNetworkUseDefault3 = {
  type: "use-default",
};
```

### `models.ManagerRetryResponseNetworkCreate3`

```typescript
const value: models.ManagerRetryResponseNetworkCreate3 = {
  type: "create",
};
```

### `models.ManagerRetryResponseNetworkByoVpcAws3`

```typescript
const value: models.ManagerRetryResponseNetworkByoVpcAws3 = {
  privateSubnetIds: [
    "<value 1>",
    "<value 2>",
    "<value 3>",
  ],
  publicSubnetIds: [],
  type: "byo-vpc-aws",
  vpcId: "<id>",
};
```

### `models.ManagerRetryResponseNetworkByoVpcGcp3`

```typescript
const value: models.ManagerRetryResponseNetworkByoVpcGcp3 = {
  networkName: "<value>",
  region: "<value>",
  subnetName: "<value>",
  type: "byo-vpc-gcp",
};
```

### `models.ManagerRetryResponseNetworkByoVnetAzure3`

```typescript
const value: models.ManagerRetryResponseNetworkByoVnetAzure3 = {
  privateSubnetName: "<value>",
  publicSubnetName: "<value>",
  type: "byo-vnet-azure",
  vnetResourceId: "<id>",
};
```

### `any`

```typescript
const value: any = "<value>";
```

