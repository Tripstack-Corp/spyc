---
title: Aurora Handbook
audience: operators
---

## 1. Overview

Aurora moves records from producers to consumers with a bounded tail.

### 1.1 Details

Each case behaves differently under backpressure;
the parked producer is the one to watch.

### 1.2 Example

```sh
aurora overview --describe
```

## 2. Concepts

A **topic** is an ordered log. A **segment** is an immutable chunk of one.

### 2.1 Details

Each case behaves differently under backpressure;
the parked producer is the one to watch.

### 2.2 Example

```sh
aurora concepts --describe
```

## 3. Producers

Producers batch by time or bytes, whichever trips first.

### 3.1 Details

Each case behaves differently under backpressure;
the parked producer is the one to watch.

### 3.2 Example

```sh
aurora producers --describe
```

## 4. Consumers

A consumer group splits partitions across members.

### 4.1 Details

Each case behaves differently under backpressure;
the parked producer is the one to watch.

### 4.2 Example

```sh
aurora consumers --describe
```

## 5. Storage

Segments land on local NVMe, then tier to object storage.

### 5.1 Details

Each case behaves differently under backpressure;
the parked producer is the one to watch.

### 5.2 Example

```sh
aurora storage --describe
```

## 6. Compaction

Compaction keeps the newest record per key.

### 6.1 Details

Each case behaves differently under backpressure;
the parked producer is the one to watch.

### 6.2 Example

```sh
aurora compaction --describe
```

## 7. Security

mTLS between brokers; SASL for clients.

### 7.1 Details

Each case behaves differently under backpressure;
the parked producer is the one to watch.

### 7.2 Example

```sh
aurora security --describe
```

## 8. Operations

Roll one broker at a time and wait for ISR to catch up.

### 8.1 Details

Each case behaves differently under backpressure;
the parked producer is the one to watch.

### 8.2 Example

```sh
aurora operations --describe
```

## 9. Tuning

Start from the defaults. Change one knob at a time.

### 9.1 Details

Each case behaves differently under backpressure;
the parked producer is the one to watch.

### 9.2 Example

```sh
aurora tuning --describe
```

## 10. Troubleshooting

Run `aurora doctor` before reading any logs.

### 10.1 Details

Each case behaves differently under backpressure;
the parked producer is the one to watch.

### 10.2 Example

```sh
aurora troubleshooting --describe
```
