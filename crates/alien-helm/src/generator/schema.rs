pub(super) fn values_schema_json() -> String {
    r##"{
  "$schema": "https://json-schema.org/draft-07/schema#",
  "type": "object",
  "additionalProperties": false,
  "properties": {
    "nameOverride": { "type": "string" },
    "fullnameOverride": { "type": "string" },
    "management": {
      "type": "object",
      "additionalProperties": false,
      "required": ["token", "updates", "telemetry", "healthChecks"],
      "properties": {
        "token": { "type": "string" },
        "existingSecret": {
          "type": "object",
          "additionalProperties": false,
          "properties": {
            "name": { "type": "string" },
            "tokenKey": { "type": "string", "minLength": 1 }
          }
        },
        "name": { "type": "string" },
        "url": { "type": "string" },
        "deploymentId": { "type": ["string", "null"] },
        "updates": { "type": "string", "enum": ["auto", "approval-required"] },
        "telemetry": { "type": "string", "enum": ["auto", "approval-required", "off"] },
        "healthChecks": { "type": "string", "enum": ["on", "off"] }
      }
    },
    "runtime": {
      "type": "object",
      "additionalProperties": false,
      "properties": {
        "image": {
          "type": "object",
          "additionalProperties": false,
          "properties": {
            "repository": { "type": "string", "minLength": 1 },
            "tag": { "type": "string", "minLength": 1 },
            "pullPolicy": { "type": "string", "enum": ["Always", "IfNotPresent", "Never"] }
          }
        },
        "imagePullSecrets": {
          "type": "array",
          "items": {
            "type": "object",
            "additionalProperties": false,
            "required": ["name"],
            "properties": { "name": { "type": "string", "minLength": 1 } }
          }
        },
        "podLabels": { "type": "object", "additionalProperties": { "type": "string" } },
        "podAnnotations": { "type": "object", "additionalProperties": { "type": "string" } },
        "automountServiceAccountToken": { "type": "boolean" },
        "encryption": {
          "type": "object",
          "additionalProperties": false,
          "properties": {
            "key": { "type": "string" },
            "existingSecret": {
              "type": "object",
              "additionalProperties": false,
              "properties": {
                "name": { "type": "string" },
                "key": { "type": "string", "minLength": 1 }
              }
            }
          }
        },
        "replicas": { "type": "integer", "minimum": 1 },
        "resources": { "type": "object" },
        "api": {
          "type": "object",
          "additionalProperties": false,
          "properties": {
            "enabled": { "type": "boolean" },
            "bindHost": { "type": "string" },
            "port": { "type": "integer", "minimum": 1, "maximum": 65535 },
            "service": {
              "type": "object",
              "additionalProperties": false,
              "properties": {
                "type": { "type": "string", "enum": ["ClusterIP", "NodePort", "LoadBalancer"] }
              }
            }
          }
        },
        "probes": {
          "type": "object",
          "additionalProperties": false,
          "properties": {
            "liveness": { "$ref": "#/definitions/httpProbe" },
            "readiness": { "$ref": "#/definitions/httpProbe" }
          }
        },
        "security": {
          "type": "object",
          "additionalProperties": false,
          "properties": {
            "podSecurityContext": { "type": "object" },
            "containerSecurityContext": { "type": "object" }
          }
        },
        "tmp": {
          "type": "object",
          "additionalProperties": false,
          "properties": {
            "enabled": { "type": "boolean" },
            "sizeLimit": { "type": "string" }
          }
        },
        "data": {
          "type": "object",
          "additionalProperties": false,
          "properties": {
            "mountPath": { "type": "string", "minLength": 1 },
            "persistence": {
              "type": "object",
              "additionalProperties": false,
              "properties": {
                "enabled": { "type": "boolean" },
                "existingClaim": { "type": "string" },
                "storageClassName": { "type": "string" },
                "accessModes": { "type": "array", "items": { "type": "string" } },
                "size": { "type": "string" }
              }
            }
          }
        },
        "scheduling": {
          "type": "object",
          "additionalProperties": false,
          "properties": {
            "nodeSelector": { "type": "object", "additionalProperties": { "type": "string" } },
            "tolerations": { "type": "array" },
            "affinity": { "type": "object" },
            "topologySpreadConstraints": { "type": "array" },
            "priorityClassName": { "type": "string" },
            "runtimeClassName": { "type": "string" }
          }
        },
        "pdb": {
          "type": "object",
          "additionalProperties": false,
          "properties": {
            "enabled": { "type": "boolean" },
            "minAvailable": { "type": ["integer", "string"] },
            "maxUnavailable": { "type": ["integer", "string"] }
          }
        },
        "networkPolicy": {
          "type": "object",
          "additionalProperties": false,
          "properties": {
            "enabled": { "type": "boolean" },
            "ingress": {
              "type": "object",
              "additionalProperties": false,
              "properties": { "enabled": { "type": "boolean" } }
            },
            "egress": {
              "type": "object",
              "additionalProperties": false,
              "properties": { "enabled": { "type": "boolean" } }
            }
          }
        },
        "cleanup": {
          "type": "object",
          "additionalProperties": false,
          "properties": {
            "onUninstall": {
              "type": "object",
              "additionalProperties": false,
              "properties": {
                "enabled": { "type": "boolean" },
                "deletePersistentVolumeClaims": { "type": "boolean" },
                "image": {
                  "type": "object",
                  "additionalProperties": false,
                  "properties": {
                    "repository": { "type": "string", "minLength": 1 },
                    "tag": { "type": "string", "minLength": 1 },
                    "pullPolicy": { "type": "string", "enum": ["Always", "IfNotPresent", "Never"] }
                  }
                }
              }
            }
          }
        }
      }
    },
    "managerServiceAccount": {
      "type": "object",
      "properties": {
        "annotations": { "type": "object", "additionalProperties": { "type": "string" } },
        "labels": { "type": "object", "additionalProperties": { "type": "string" } }
      }
    },
    "logCollector": {
      "type": "object",
      "additionalProperties": false,
      "properties": {
        "enabled": { "type": "boolean" },
        "token": { "type": "string" },
        "image": {
          "type": "object",
          "additionalProperties": false,
          "properties": {
            "repository": { "type": "string", "minLength": 1 },
            "tag": { "type": "string", "minLength": 1 },
            "pullPolicy": { "type": "string", "enum": ["Always", "IfNotPresent", "Never"] }
          }
        },
        "resources": { "type": "object" },
        "scope": {
          "type": "object",
          "additionalProperties": false,
          "properties": {
            "deploymentLabelKey": { "type": "string" },
            "deploymentLabelValue": { "type": "string" }
          }
        }
      }
    },
    "serviceAccounts": {
      "type": "object",
      "additionalProperties": {
        "type": "object",
        "properties": {
          "annotations": { "type": "object", "additionalProperties": { "type": "string" } },
          "labels": { "type": "object", "additionalProperties": { "type": "string" } },
          "rbac": {
            "type": "object",
            "additionalProperties": false,
            "properties": {
              "rules": {
                "type": "array",
                "items": {
                  "type": "object",
                  "additionalProperties": false,
                  "properties": {
                    "apiGroups": {
                      "type": "array",
                      "items": { "type": "string" }
                    },
                    "resources": {
                      "type": "array",
                      "items": { "type": "string", "minLength": 1 }
                    },
                    "verbs": {
                      "type": "array",
                      "items": { "type": "string", "minLength": 1 }
                    }
                  },
                  "required": ["apiGroups", "resources", "verbs"]
                }
              }
            }
          }
        }
      }
    },
    "stackSettings": {
      "type": ["object", "null"],
      "properties": {
        "deploymentModel": { "type": "string", "enum": ["pull", "Pull"] },
        "updates": { "type": "string" },
        "telemetry": { "type": "string" },
        "heartbeats": { "type": "string" }
      },
      "additionalProperties": true
    },
    "infrastructure": { "type": ["object", "null"] },
    "basePlatform": { "type": ["string", "null"], "enum": ["aws", "gcp", "azure", null] },
    "basePlatformConfig": {
      "type": "object",
      "additionalProperties": false,
      "properties": {
        "gcp": {
          "type": "object",
          "additionalProperties": false,
          "properties": {
            "projectId": { "type": "string" },
            "region": { "type": "string" }
          }
        },
        "aws": {
          "type": "object",
          "additionalProperties": false,
          "properties": {
            "region": { "type": "string" }
          }
        },
        "azure": {
          "type": "object",
          "additionalProperties": false,
          "properties": {
            "location": { "type": "string" },
            "subscriptionId": { "type": "string" },
            "tenantId": { "type": "string" }
          }
        }
      }
    },
    "heartbeat": {
      "type": "object",
      "additionalProperties": false,
      "properties": {
        "collection": {
          "type": "object",
          "additionalProperties": false,
          "properties": {
            "nodes": {
              "type": "object",
              "additionalProperties": false,
              "properties": {
                "enabled": { "type": "boolean" }
              }
            }
          }
        }
      }
    },
    "clusterBootstrap": {
      "type": "object",
      "additionalProperties": false,
      "properties": {
        "metricsServer": {
          "type": "object",
          "additionalProperties": false,
          "properties": {
            "enabled": { "type": "boolean" },
            "image": { "type": "string" }
          }
        },
        "storageClass": {
          "type": "object",
          "additionalProperties": false,
          "properties": {
            "default": {
              "type": "object",
              "additionalProperties": false,
              "properties": {
                "enabled": { "type": "boolean" },
                "name": { "type": "string" },
                "provisioner": { "type": "string" },
                "parameters": { "type": "object", "additionalProperties": { "type": "string" } }
              }
            }
          }
        },
        "ingress": {
          "type": "object",
          "additionalProperties": false,
          "properties": {
            "eksAutoMode": {
              "type": "object",
              "additionalProperties": false,
              "properties": {
                "enabled": { "type": "boolean" },
                "name": { "type": "string" },
                "controller": { "type": "string" },
                "scheme": { "type": "string" },
                "subnetIds": {
                  "type": "array",
                  "items": { "type": "string" }
                }
              }
            },
            "azureApplicationGatewayForContainers": {
              "type": "object",
              "additionalProperties": false,
              "properties": {
                "enabled": { "type": "boolean" },
                "applicationLoadBalancer": {
                  "type": "object",
                  "additionalProperties": false,
                  "properties": {
                    "name": { "type": "string" },
                    "namespace": { "type": "string" },
                    "associationSubnetId": { "type": "string" }
                  }
                }
              }
            }
          }
        },
        "compute": {
          "type": "object",
          "additionalProperties": false,
          "properties": {
            "eksAutoMode": {
              "type": "object",
              "additionalProperties": false,
              "properties": {
                "arm64NodePool": {
                  "type": "object",
                  "additionalProperties": false,
                  "properties": {
                    "enabled": { "type": "boolean" },
                    "name": { "type": "string" },
                    "nodeClassName": { "type": "string" },
                    "capacityType": { "type": "string" },
                    "instanceCategories": {
                      "type": "array",
                      "items": { "type": "string" }
                    },
                    "minInstanceGeneration": { "type": "string" },
                    "limits": {
                      "type": "object",
                      "additionalProperties": false,
                      "properties": {
                        "cpu": { "type": "string" },
                        "memory": { "type": "string" }
                      }
                    }
                  }
                }
              }
            }
          }
        }
      }
    },
    "serviceAccountPrefix": { "type": "string" },
    "services": {
      "type": "object",
      "additionalProperties": {
        "type": "object",
        "additionalProperties": false,
        "properties": {
          "type": { "type": "string", "enum": ["clusterIp", "loadBalancer"] },
          "port": { "type": "integer", "minimum": 1, "maximum": 65535 },
          "targetPort": { "type": "integer", "minimum": 1, "maximum": 65535 },
          "component": { "type": "string" }
        }
      }
    },
    "publicEndpoints": {
      "type": "object",
      "additionalProperties": {
        "type": "object",
        "additionalProperties": { "type": "string" }
      }
    },
    "persistentStorage": { "type": "object" },
    "ephemeralStorage": { "type": "object" }
  },
  "definitions": {
    "httpProbe": {
      "type": "object",
      "additionalProperties": false,
      "properties": {
        "enabled": { "type": "boolean" },
        "path": { "type": "string", "minLength": 1 },
        "initialDelaySeconds": { "type": "integer", "minimum": 0 },
        "periodSeconds": { "type": "integer", "minimum": 1 },
        "timeoutSeconds": { "type": "integer", "minimum": 1 },
        "failureThreshold": { "type": "integer", "minimum": 1 }
      }
    }
  },
  "oneOf": [
    {
      "title": "registered setup",
      "required": ["management"],
      "properties": {
        "management": {
          "required": ["token", "deploymentId"],
          "properties": {
            "deploymentId": { "type": "string", "minLength": 1 }
          }
        },
        "infrastructure": { "type": "null" }
      }
    },
    {
      "title": "external-bindings initialize path",
      "required": ["management", "infrastructure"],
      "properties": {
        "management": {
          "properties": {
            "deploymentId": { "type": "null" }
          }
        },
        "stackSettings": { "type": ["object", "null"] },
        "infrastructure": { "type": "object" }
      }
    }
  ]
}
"##
    .to_string()
}
