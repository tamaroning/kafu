apiVersion: v1
kind: Pod
metadata:
  name: kafu-server
  labels:
    component: kafu-server
    kafu-service-name: "{config_name}"
    kafu-node-id: "{node_id}"
{{ if instance_id }}    kafu-instance: "{instance_id}"
{{ endif }}spec:
  nodeSelector:
    kafu-node: {placement}
  containers:
  - name: kafu-server
    command:
    - kafu_serve
    - --node-id
    - "{node_id}"
    - /etc/kafu/kafu-config.yaml
  volumes:
  - name: kafu-config
    configMap:
      name: "{configmap_name}"
---
apiVersion: v1
kind: Service
metadata:
  name: kafu-server
spec:
  selector:
    component: kafu-server
    kafu-service-name: "{config_name}"
    kafu-node-id: "{node_id}"
{{ if instance_id }}    kafu-instance: "{instance_id}"
{{ endif }}
