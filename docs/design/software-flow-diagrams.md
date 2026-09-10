# Software Flow Diagrams

Status: draft · Date: 2026-09-10

Companion to [`architecture.md`](architecture.md). These diagrams visualize the system architecture, data flows, and operational sequences.

## 1. System Architecture Overview

```mermaid
graph TB
    subgraph "External Inputs"
        PAD[Gamepad<br/>BLE/USB]
        PHONE[Phone APP<br/>BLE]
        LAPTOP[Laptop<br/>ssh/WebRTC]
        PEER[Remote peer<br/>WebRTC]
        GITHUB[GitHub Release<br/>https]
    end

    subgraph "Transport daemons"
        padd[padd<br/>gamepad → intents]
        btd[btd<br/>BLE transport]
        mediad[mediad<br/>camera/mic<br/>WebRTC gateway]
    end

    subgraph "Core daemons"
        robotd[robotd<br/>50Hz control loop<br/>motors/IMU/policy/safety]
        configd[configd<br/>WiFi/identity/pairing PIN]
        updaterd[updaterd<br/>signature verify/atomic swap<br/>health gate/rollback]
        tofd[tofd<br/>8x8 ToF depth sensor]
    end

    subgraph "Hardware"
        SERVOS[15 servos]
        IMU[IMU]
        CAMERA[Camera]
        MIC[Microphone]
        SPEAKER[Speaker]
        TOF[ToF sensor]
        BT_RADIO[Bluetooth radio]
    end

    PAD -->|BLE/USB| padd
    PHONE -->|BLE| btd
    LAPTOP -->|ssh/WebRTC| mediad
    PEER -->|WebRTC| mediad
    GITHUB -->|download| updaterd

    padd -->|unix socket<br/>JSON-RPC| robotd
    btd -->|unix socket<br/>JSON-RPC| robotd
    btd -->|unix socket<br/>JSON-RPC| configd
    btd -->|unix socket<br/>JSON-RPC| updaterd
    mediad -->|unix socket<br/>JSON-RPC| robotd
    mediad -->|unix socket<br/>JSON-RPC| configd
    mediad -->|unix socket<br/>JSON-RPC| updaterd

    tofd -->|publishes frames| mediad
    tofd -->|publishes frames| robotd

    robotd -->|Dynamixel<br/>UART| SERVOS
    robotd -->|Dynamixel<br/>UART| IMU
    mediad -->|V4L2| CAMERA
    mediad -->|ALSA| MIC
    mediad -->|ALSA| SPEAKER
    tofd -->|I2C| TOF
    btd -->|BlueZ| BT_RADIO
    configd -->|D-Bus| BT_RADIO

    style robotd fill:#ff9,stroke:#333,stroke-width:2px
    style updaterd fill:#9f9,stroke:#333,stroke-width:2px
    style configd fill:#9f9,stroke:#333,stroke-width:2px
    style btd fill:#9f9,stroke:#333,stroke-width:2px
```

**Key points:**
- `robotd` is the only daemon that touches motors — all others send intents
- Three daemons survive a dead `robotd`: `configd`, `updaterd`, `btd` — they are the recovery path
- `btd`, `padd`, `mediad` own nothing — they are transports
- `tofd` publishes frames and reads nothing

## 2. Release Installation / Update Flow

```mermaid
sequenceDiagram
    participant User as User/CI
    participant GitHub as GitHub Release
    participant Updaterd as updaterd
    participant Robotd as robotd
    participant FS as Filesystem

    User->>GitHub: publish release (signed)
    User->>Updaterd: robotctl update apply

    Note over Updaterd: Preflight
    Updaterd->>Robotd: safeToRestart()?
    Robotd-->>Updaterd: yes/no
    Updaterd->>Updaterd: active WebRTC session?

    alt unsafe or session active
        Updaterd-->>User: retry later
    else safe to proceed
        Updaterd->>GitHub: download manifest + artifact
        GitHub-->>Updaterd: manifest.json + .tar.zst
        
        Note over Updaterd: Verification
        Updaterd->>Updaterd: verify manifest signature
        Updaterd->>Updaterd: verify artifact sha256
        Updaterd->>Updaterd: verify artifact signature
        Updaterd->>Updaterd: check version compatibility

        Note over Updaterd: Installation
        Updaterd->>FS: extract to /releases/<version>/
        Updaterd->>FS: move current symlink
        Updaterd->>FS: append to update-log.jsonl
        
        Note over Updaterd: Restart
        Updaterd->>Updaterd: systemctl restart robotd configd padd
        Updaterd-->>User: reply (restarts self in 5s)
        Updaterd->>Updaterd: systemctl restart updaterd btd

        Note over Updaterd: Health gate
        loop wait for health
            Updaterd->>Robotd: robot.health?
            alt healthy
                Updaterd->>FS: mark success
            else unhealthy or timeout
                Updaterd->>FS: restore old symlink
                Updaterd->>Updaterd: systemctl restart (rollback)
                Updaterd-->>User: rollback notification
            end
        end
    end
```

**Key points:**
- Signatures and hashes verified before any filesystem changes
- Atomic symlink swap — either the whole release is live or none of it
- Health gate with automatic rollback on failure
- `updaterd` restarts itself last (cannot restart mid-update)

## 3. Daemon Startup Sequence

```mermaid
flowchart TD
    START[System boot] --> SYSINIT[systemd initialization]
    SYSINIT --> CREATE_GROUP[create robot group<br/>sysusers.d]
    
    CREATE_GROUP --> UPDATERD[start updaterd]
    UPDATERD --> ROBOTD[start robotd]
    
    ROBOTD --> |success| CONFIGD[start configd]
    ROBOTD --> |failure| ROBOTD_RETRY[retry / report unhealthy]
    
    CONFIGD --> BTD[start btd<br/>needs configd's PIN]
    BTD --> |success| PADD[start padd]
    BTD --> |failure<br/>no bluetooth| PADD
    PADD --> |success| MEDIAD[start mediad]
    PADD --> |failure<br/>no gamepad| MEDIAD
    
    MEDIAD --> |success| TOFD[start tofd]
    MEDIAD --> |failure<br/>no camera| DONE
    TOFD --> DONE[startup complete]
    
    DONE --> BOOT_CHECK[3 minutes later<br/>robot-boot-check.timer]
    BOOT_CHECK --> CHECK_DAEMONS{all daemons<br/>running?}
    CHECK_DAEMONS -->|yes| NORMAL[normal operation]
    CHECK_DAEMONS -->|no| FALLBACK[rollback to golden release]
    
    style UPDATERD fill:#9f9
    style ROBOTD fill:#ff9
    style CONFIGD fill:#9f9
    style BTD fill:#9f9
    style PADD fill:#9f9
    style MEDIAD fill:#9f9
    style TOFD fill:#9f9
    style FALLBACK fill:#f99
```

**Key points:**
- `configd` before `btd`: btd asks configd for the pairing PIN
- `mediad` last: it forwards calls to three other daemons
- `btd`/`padd`/`mediad` allowed to fail — robot still walks without them
- Boot check timer fires 3 minutes after boot, rolls back if daemons didn't come up

## 4. Control Loop (Gamepad → Motors)

```mermaid
flowchart LR
    subgraph "Input device"
        STICKS[Joysticks]
        BUTTONS[Buttons]
    end
    
    subgraph "padd"
        READ[Read input]
        MAP[Map to intents]
        SEND[Send to robotd]
    end
    
    subgraph "robotd control loop (50Hz)"
        RECV[Receive intents]
        CACHE[Cache latest value]
        
        subgraph "Each tick"
            READ_SENSORS[Read sensors]
            OBS[Build observations]
            POLICY[Run ONNX policy]
            SAFETY[Safety checks]
            BUS[Write to Dynamixel bus]
        end
    end
    
    subgraph "Hardware"
        SERVOS[Servos]
        IMU[IMU]
    end
    
    STICKS --> READ
    BUTTONS --> READ
    READ --> MAP
    MAP -->|velocity/gaze/sit| SEND
    SEND -->|unix socket| RECV
    RECV --> CACHE
    
    CACHE --> OBS
    READ_SENSORS --> OBS
    IMU --> READ_SENSORS
    
    OBS --> POLICY
    POLICY -->|joint targets| SAFETY
    SAFETY -->|clamp/e-stop| BUS
    BUS --> SERVOS
    
    style POLICY fill:#f9f
    style SAFETY fill:#f99
    style BUS fill:#9ff
```

**Key points:**
- 50Hz tick rate — real-time enough for walking
- `robotd` subscribes to intents, reads a cached latest value (non-blocking)
- Safety layer clamps joint limits, detects falls, can e-stop
- Policy output is joint targets; safety decides what's executable

## 5. First-time Provisioning Flow

```mermaid
flowchart TD
    subgraph "Phase 1: Board setup"
        A1[check_environment] --> A2[wait_for_clock<br/>NTP sync]
        A2 --> A3[configure_overlay<br/>uart2-m0]
        A3 --> A4[free_motor_port<br/>disable serial-getty]
        A4 --> A5[configure_bluetooth<br/>aic8800 workaround]
        A5 --> A6[configure_audio<br/>TLV320AIC3104 codec]
        A6 --> A7[configure_tof<br/>I2C udev rule]
        A7 --> A8[configure_camera<br/>device-tree overlay]
        A8 --> A9[install_onnxruntime<br/>1.28.0]
    end
    
    A9 --> REBOOT{needs reboot?}
    REBOOT -->|yes| DO_REBOOT[sudo reboot]
    REBOOT -->|no| PHASE2
    
    DO_REBOOT --> PHASE2
    
    subgraph "Phase 2: WiFi migration"
        B1[check netplan] --> B2[arm backstop<br/>restore on failure]
        B2 --> B3[read netplan config]
        B3 --> B4[create NM profile]
        B4 --> B5[test connection]
        B5 --> B6{success?}
        B6 -->|yes| B7[disable backstop]
        B6 -->|no| B8[restore netplan<br/>retry next time]
    end
    
    B7 --> PHASE2
    
    subgraph "Phase 3: Daemon install"
        C1[resolve_bootstrap_asset<br/>get latest release]
        C1 --> C2[install_config<br/>updater.toml + public keys]
        C2 --> C3[install_dev_key<br/>optional]
        C3 --> C4[bootstrap_first_release<br/>download & verify updaterd]
        C4 --> C5[create_group<br/>robot group + service accounts]
        C5 --> C6[install_units<br/>start daemons in order]
        C6 --> C7[install_token_dropin<br/>optional]
        C7 --> C8[verify_install<br/>check status]
    end
    
    PHASE2[continue] --> C1
    C8 --> DONE[install complete<br/>robotctl health]
    
    style A3 fill:#ff9
    style A9 fill:#ff9
    style C4 fill:#9f9
    style C8 fill:#9f9
```

**Key points:**
- Phase 1 (`setup-board.sh`) is OS-level: overlays, ONNX Runtime, audio codec
- Phase 2 (`migrate-network.sh`) moves from netplan to NetworkManager
- Phase 3 (`install.sh`) installs the daemon release via the real update engine
- Each phase is idempotent and can be re-run independently

## 6. IPC Communication Model

```mermaid
flowchart TB
    subgraph "Unix Sockets (JSON-RPC 2.0, NDJSON)"
        SOCK_ROBOTD[/run/robotd.sock]
        SOCK_CONFIGD[/run/configd.sock]
        SOCK_UPDATERD[/run/updaterd.sock]
        SOCK_TOFD[/run/tofd/tof.sock]
    end
    
    subgraph "Request/Response"
        REQ[request<br/>id + method + params]
        RESP[response<br/>id + result/error]
    end
    
    subgraph "Notifications (subscriptions)"
        NOTIF[notification<br/>method + params<br/>no id]
    end
    
    subgraph "Clients"
        ROBOTCTL[robotctl]
        PADD_CLIENT[padd]
        BTD_CLIENT[btd]
        MEDIAD_CLIENT[mediad]
    end
    
    subgraph "Services"
        ROBOTD[robotd]
        CONFIGD[configd]
        UPDATERD[updaterd]
        TOFD[tofd]
    end
    
    ROBOTCTL -->|robot.health| SOCK_ROBOTD
    PADD_CLIENT -->|robot.intent| SOCK_ROBOTD
    BTD_CLIENT -->|robot.*| SOCK_ROBOTD
    BTD_CLIENT -->|net.*| SOCK_CONFIGD
    BTD_CLIENT -->|update.*| SOCK_UPDATERD
    MEDIAD_CLIENT -->|robot.*| SOCK_ROBOTD
    
    ROBOTD --- SOCK_ROBOTD
    CONFIGD --- SOCK_CONFIGD
    UPDATERD --- SOCK_UPDATERD
    TOFD --- SOCK_TOFD
    
    SOCK_ROBOTD --> REQ
    REQ --> ROBOTD
    ROBOTD --> RESP
    RESP --> SOCK_ROBOTD
    
    TOFD -->|tof.stream| NOTIF
    NOTIF --> SOCK_TOFD
    SOCK_TOFD --> MEDIAD_CLIENT
    
    style SOCK_ROBOTD fill:#9cf
    style SOCK_CONFIGD fill:#9cf
    style SOCK_UPDATERD fill:#9cf
    style SOCK_TOFD fill:#9cf
```

**Key points:**
- One socket per service — no broker, no bus
- JSON-RPC 2.0 with NDJSON framing (one object per line)
- Requests have `id`, responses correlate by `id`
- Notifications have no `id` — used for subscriptions and event streams
- `tofd` publishes frames as a stream of notifications

## 7. Security & Authorization Model

```mermaid
flowchart TD
    subgraph "Socket permissions"
        SOCK[Unix Socket<br/>mode 0660<br/>root:robot]
    end
    
    subgraph "Group membership"
        ROBOT_GRP[robot group]
        ROOT[root]
    end
    
    subgraph "Access control"
        CAN_TALK[can connect<br/>read status]
        CAN_MUTATE[can modify<br/>robot state]
    end
    
    subgraph "allow_uids / allow_gids"
        ALLOW_LIST[configured users/groups]
    end
    
    subgraph "SO_PEERCRED"
        PEERCRED[get caller uid/gid/pid]
    end
    
    subgraph "Clients"
        READONLY[read-only client<br/>robotctl status]
        MUTATING[mutating client<br/>robotctl update apply]
        BTD_CLIENT[btd<br/>BLE relay]
    end
    
    ROOT --> CAN_TALK
    ROOT --> CAN_MUTATE
    ROBOT_GRP --> CAN_TALK
    
    CAN_TALK --> PEERCRED
    PEERCRED --> CHECK{in allow list?}
    CHECK -->|yes| CAN_MUTATE
    CHECK -->|no| READONLY
    
    ALLOW_LIST -.-> CHECK
    
    BTD_CLIENT -->|needs allow_uids| CAN_MUTATE
    
    style CAN_MUTATE fill:#f99
    style READONLY fill:#9f9
    style CHECK fill:#ff9
```

**Key points:**
- Socket mode 0660 + `robot` group: only group members can connect
- `SO_PEERCRED` identifies the caller (uid/gid/pid)
- `allow_uids`/`allow_gids` in config: who may mutate robot state
- Read-only access granted to all `robot` group members
- `btd` must be in `allow_uids` to relay update requests from BLE

## 8. State Ownership & Persistence

```mermaid
flowchart TB
    subgraph "Per-board config (survives update + rollback)"
        ETC_ROBOT[/etc/robot/]
        UPDATER_TOML[updater.toml]
        ROBOTD_TOML[robotd.toml]
        TRUSTED_KEYS[trusted_keys/]
        CONFIG_JSON[config.json<br/>name + PIN]
    end
    
    subgraph "Release artifacts (swapped atomically)"
        OPT_ROBOT[/opt/robot/daemon/]
        RELEASES[releases/<version>/]
        CURRENT[current -> releases/<version>]
    end
    
    subgraph "Runtime state (survives reboot)"
        VAR_LIB[/var/lib/robot/]
        UPDATE_LOG[update-log.jsonl]
        BOOT_COUNTER[boot-counter]
        LOCK[lock file]
    end
    
    subgraph "Volatile state (lost on power cut)"
        VAR_LOG[/var/log<br/>zram device]
        JOURNAL[journald]
    end
    
    ETC_ROBOT --> UPDATER_TOML
    ETC_ROBOT --> ROBOTD_TOML
    ETC_ROBOT --> TRUSTED_KEYS
    ETC_ROBOT --> CONFIG_JSON
    
    OPT_ROBOT --> RELEASES
    OPT_ROBOT --> CURRENT
    
    VAR_LIB --> UPDATE_LOG
    VAR_LIB --> BOOT_COUNTER
    VAR_LIB --> LOCK
    
    VAR_LOG --> JOURNAL
    
    style ETC_ROBOT fill:#9f9
    style VAR_LIB fill:#9f9
    style OPT_ROBOT fill:#ff9
    style VAR_LOG fill:#f99
```

**Key points:**
- `/etc/robot/`: per-board config, never overwritten by updates
- `/opt/robot/daemon/releases/`: release artifacts, swapped atomically
- `/var/lib/robot/`: durable runtime state (fsynced)
- `/var/log`: zram on this image — journal survives reboot but not power cut
- Update history is the durable record, not the journal
