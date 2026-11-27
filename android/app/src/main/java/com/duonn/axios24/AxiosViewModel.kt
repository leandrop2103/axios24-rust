package com.duonn.axios24

import android.app.Application
import android.content.Context
import android.net.wifi.WifiManager
import android.os.Build
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.mutableStateListOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import androidx.lifecycle.AndroidViewModel
import androidx.lifecycle.viewModelScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.Job
import kotlinx.coroutines.delay
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import java.net.DatagramPacket
import java.net.DatagramSocket
import java.net.InetAddress
import java.net.InetSocketAddress
import java.net.Socket
import java.util.concurrent.LinkedBlockingQueue
import java.util.concurrent.atomic.AtomicBoolean
import java.util.concurrent.atomic.AtomicInteger
import java.util.concurrent.atomic.AtomicLong
import kotlin.math.pow
import kotlin.math.roundToInt
import kotlin.math.sin

sealed class SelectedChannel {
    data class Input(val ch: Int) : SelectedChannel() // 0..24 (24 = DIGI)
    data class Fx(val fx: Int) : SelectedChannel()    // 0..1
    data class Output(val out: Int) : SelectedChannel() // 0..9 (8=MAIN L, 9=MAIN R)
    data class Dca(val dca: Int) : SelectedChannel()  // 0..3

    fun groupAndIndex(): Pair<Int, Int> = when (this) {
        is Input -> 1 to ch
        is Fx -> 2 to fx
        is Output -> 3 to out
        is Dca -> 4 to dca
    }

    fun flatId(): Int = when (this) {
        is Input -> ch.coerceIn(0, 25)
        is Fx -> 26 + fx.coerceIn(0, 1)
        is Output -> 28 + out.coerceIn(0, 9)
        is Dca -> 38 + dca.coerceIn(0, 3)
    }

    fun shortBadge(): String = when (this) {
        is Input -> if (ch == 24) "DIGI" else "CH${ch + 1}"
        is Fx -> "FX${fx + 1}"
        is Output -> when (out) {
            8 -> "MAIN L"
            9 -> "MAIN R"
            else -> "AUX${out + 1}"
        }
        is Dca -> "DCA${dca + 1}"
    }

    companion object {
        fun fromFlatId(id: Int): SelectedChannel = when (id) {
            in 0..25 -> Input(id)
            in 26..27 -> Fx(id - 26)
            in 28..37 -> Output(id - 28)
            else -> Dca((id - 38).coerceIn(0, 3))
        }
    }
}

enum class FaderLayer(val label: String) {
    CH1_8("CH 1-8"),
    CH9_16("CH 9-16"),
    CH17_24("CH 17-24"),
    DIGI_FX("DIGI / FX"),
    AUX1_8("AUX 1-8"),
    DCA1_4("DCA 1-4"),
    ALL_INPUTS("TODOS 1-24");

    fun channels(): List<SelectedChannel> = when (this) {
        CH1_8 -> (0..7).map { SelectedChannel.Input(it) }
        CH9_16 -> (8..15).map { SelectedChannel.Input(it) }
        CH17_24 -> (16..23).map { SelectedChannel.Input(it) }
        DIGI_FX -> listOf(
            SelectedChannel.Input(24),
            SelectedChannel.Fx(0),
            SelectedChannel.Fx(1)
        )
        AUX1_8 -> (0..7).map { SelectedChannel.Output(it) }
        DCA1_4 -> (0..3).map { SelectedChannel.Dca(it) }
        ALL_INPUTS -> (0..23).map { SelectedChannel.Input(it) }
    }
}

enum class MainTab(val label: String) {
    MIXER("Mixer"),
    EDIT("Editar Canal"),
    EQ("EQ / HPF"),
    FX_BT("Efeitos & BT"),
    SCENES("Cenas & Rede")
}

sealed class SendsOnFaderTarget(val label: String) {
    data object MainLR : SendsOnFaderTarget("MAIN L/R")
    data class Aux(val aux: Int) : SendsOnFaderTarget("AUX ${aux + 1}")
    data class Fx(val fx: Int) : SendsOnFaderTarget("FX ${fx + 1}")
}

enum class ChannelPreset(val label: String, val gainDb: Float, val hpfHz: Float, val gateTh: Float, val compTh: Float, val compRatio: Float) {
    VOZ_LIDER("Voz Líder / Pastor", 26f, 100f, -42f, -16f, 3.5f),
    BACKING_VOCAL("Backing Vocal", 24f, 115f, -40f, -18f, 4.0f),
    VIOLAO("Violão Aço / Nylon", 22f, 95f, -48f, -18f, 3.0f),
    GUITARRA("Guitarra Amp/Linha", 20f, 85f, -45f, -16f, 3.5f),
    BAIXO("Contrabaixo", 18f, 35f, -48f, -14f, 4.5f),
    TECLADO("Teclado / Piano", 16f, 60f, -55f, -18f, 2.5f),
    BUMBO("Bumbo (Kick)", 24f, 30f, -34f, -14f, 4.5f),
    CAIXA("Caixa (Snare)", 22f, 80f, -36f, -16f, 4.0f)
}

class AxiosViewModel(app: Application) : AndroidViewModel(app) {
    private val prefs = app.getSharedPreferences("axios24_prefs", Context.MODE_PRIVATE)

    val regs = IntArray(NUM_REGS) { idx -> defaultRegValue(idx) }
    val scenes = Array(16) { regs.copyOf() }
    val sceneNames = mutableStateListOf<String>().apply {
        for (i in 1..16) add(if (i == 1) "Culto Padrão" else "Cena $i")
    }
    val channelNames = mutableStateListOf<String>().apply {
        for (i in 0 until 60) {
            add(
                when (i) {
                    in 0..23 -> "CH${i + 1}"
                    24, 25 -> "DIGI/BT"
                    26 -> "FX 1"
                    27 -> "FX 2"
                    in 28..35 -> "AUX ${i - 27}"
                    36 -> "MAIN L"
                    37 -> "MAIN R"
                    in 38..41 -> "DCA ${i - 37}"
                    else -> "CH${i + 1}"
                }
            )
        }
    }
    val channelMetersDb = mutableStateListOf<Float>().apply { repeat(60) { add(-90f) } }
    val channelClipHold = mutableStateListOf<Boolean>().apply { repeat(60) { add(false) } }
    var lastUnknownPacketInfo by mutableStateOf("Nenhum pacote extra recebido")

    fun channelMeterDb(sel: SelectedChannel): Float =
        channelMetersDb.getOrElse(sel.flatId()) { -90f }

    fun channelClipped(sel: SelectedChannel): Boolean =
        channelClipHold.getOrElse(sel.flatId()) { false }

    fun clearClipHold(sel: SelectedChannel) {
        val id = sel.flatId()
        if (id in 0 until channelClipHold.size) channelClipHold[id] = false
    }
    val safeUnplugged = mutableStateListOf<Boolean>().apply { repeat(25) { add(false) } }
    private val savedFaderBeforeUnplug = FloatArray(25) { 0f }

    var uiTick by mutableIntStateOf(0)
        private set

    var activeTab by mutableStateOf(MainTab.MIXER)
    var layer by mutableStateOf(FaderLayer.CH1_8)
    var selectedChannel by mutableStateOf<SelectedChannel>(SelectedChannel.Input(0))
    var sendsOnFader by mutableStateOf<SendsOnFaderTarget>(SendsOnFaderTarget.MainLR)
    var dcaSpill by mutableStateOf<Int?>(null)
    var firmwareLayout by mutableStateOf(FirmwareLayout.AX24E_2511)
    var connectIp by mutableStateOf("172.14.10.1")
    var connectionStatus by mutableStateOf("MODO DEMO (Offline)")
    var isConnected by mutableStateOf(false)
    var isScanning by mutableStateOf(false)
    var localPhoneIp by mutableStateOf(AxiosProtocol.detectLocalIpv4() ?: "Desconhecido")
    val discoveredDevices = mutableStateListOf<DiscoveredDevice>()
    var statusToast by mutableStateOf<String?>(null)
    var mediaSourceId by mutableIntStateOf(1) // 0=USB, 1=Bluetooth, 2=Optical, 3=Coaxial
    var personalMonitorLocked by mutableStateOf(false)
    var personalMonitorAux by mutableIntStateOf(0)
    var unlockPinInput by mutableStateOf("")

    private val txQueue = LinkedBlockingQueue<ByteArray>()
    private val netRunning = AtomicBoolean(false)
    private val connToken = AtomicInteger(0)
    private val lastHwRxMs = AtomicLong(0L)
    private val lastVuRxMs = AtomicLong(0L)
    private var netJob: Job? = null
    private var desiredProto: String? = null // "UDP" or "TCP"
    private var wifiLock: WifiManager.WifiLock? = null

    /**
     * Exact hardware VU meter registers from AX24E.25.11.01.001.exe (@ 0x00461200 & 0x00427DEC):
     * - 0x02..0x0E (13 regs): Inputs 0..25 (CH1..CH24 + DIGI L/R), 2 signed int8 dB bytes/reg (low=even, high=odd)
     * - 0x29..0x2A (2 regs):  FX 1..2, low signed int8 dB byte
     * - 0x2B..0x2F (5 regs):  Outputs 0..9 (AUX1..AUX8 + MAIN L/R), 2 signed int8 dB bytes/reg (low=even, high=odd)
     */
    private val vuMeterRegs: List<Int> =
        (0x02..0x0E).toList() + listOf(0x29, 0x2A) + (0x2B..0x2F).toList()

    @Suppress("DEPRECATION")
    private fun acquireWifiLock() {
        try {
            if (wifiLock == null) {
                val wm = getApplication<Application>().applicationContext
                    .getSystemService(Context.WIFI_SERVICE) as? WifiManager
                val mode = if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.Q) {
                    WifiManager.WIFI_MODE_FULL_LOW_LATENCY
                } else {
                    WifiManager.WIFI_MODE_FULL_HIGH_PERF
                }
                wifiLock = wm?.createWifiLock(mode, "Axios24:MixerWifiLock")?.apply {
                    setReferenceCounted(false)
                }
            }
            wifiLock?.takeIf { !it.isHeld }?.acquire()
        } catch (_: Exception) {
        }
    }

    private fun releaseWifiLock() {
        try {
            wifiLock?.takeIf { it.isHeld }?.release()
        } catch (_: Exception) {
        }
    }

    override fun onCleared() {
        super.onCleared()
        disconnect()
    }

    init {
        for (ch in 0..25) {
            regs[AxiosProtocol.mapReg(1, ch, 0xEB)] = 1      // Unmuted (0=MUTED, 1=ON)
            regs[AxiosProtocol.mapReg(1, ch, 0x09)] = 1      // Phase Normal (0=INVERTED, 1=NORMAL)
            regs[AxiosProtocol.mapReg(1, ch, 0x0B)] = 100    // Pan Center (0..200)
            regs[AxiosProtocol.mapReg(1, ch, 0x15)] = 0x04B0 // Routed to MAIN L
            regs[AxiosProtocol.mapReg(1, ch, 0x16)] = 0x04B0 // Routed to MAIN R
            regs[AxiosProtocol.mapReg(1, ch, 0x02)] = 1      // Gate enabled (threshold -80dB = 0x50)
            regs[AxiosProtocol.mapReg(1, ch, 0x03)] = 80
            regs[AxiosProtocol.mapReg(1, ch, 0x1D)] = 1      // Comp enabled (ratio 1.0, thresh 0dB)
            regs[AxiosProtocol.mapReg(1, ch, 0x1E)] = 100
            regs[AxiosProtocol.mapReg(1, ch, 0x24)] = 1      // PEQ active (not bypassed)
            for (b in 0..3) {
                regs[AxiosProtocol.mapReg(1, ch, 0x2A + b * 4 + 2)] = 500 // 0.0 dB
            }
        }
        for (fx in 0..1) {
            regs[AxiosProtocol.mapReg(2, fx, 0xEB)] = 1
            regs[AxiosProtocol.mapReg(2, fx, 0xB5)] = 0x04B0
            regs[AxiosProtocol.mapReg(2, fx, 0xB6)] = 0x04B0
            regs[AxiosProtocol.mapReg(2, fx, 0xB9)] = 1
        }
        for (out in 0..9) {
            regs[AxiosProtocol.mapReg(3, out, 0xEB)] = 1     // Unmuted (0=MUTED, 1=ON)
            regs[AxiosProtocol.mapReg(3, out, 0x3F)] = 1     // Phase Normal (0=INVERTED, 1=NORMAL)
            regs[AxiosProtocol.mapReg(3, out, 0x44)] = 1     // Comp active
            regs[AxiosProtocol.mapReg(3, out, 0x45)] = 100
            regs[AxiosProtocol.mapReg(3, out, 0x4D)] = 1     // PEQ active
            for (b in 0..6) {
                regs[AxiosProtocol.mapReg(3, out, 0x53 + b * 4 + 2)] = 500 // 0.0 dB
            }
        }
        loadUserPrefs()
        startMeterSimulationLoop()
        scanNetworkDevices()
    }

    private fun defaultRegValue(idx: Int): Int {
        // Initialize default 0dB faders on MAIN L/R (0x09F4/0x0A61) and stereo link on MAIN L/R (0xBCC bit 4)
        return when (idx) {
            0x0BCC -> 1 shl 4
            0x09F4, 0x0A61 -> 0x04B0 // 0.0 dB on MAIN L & MAIN R faders (param 0xEA)
            else -> 0
        }
    }

    fun toast(msg: String) {
        statusToast = msg
    }

    fun getReg(reg: Int): Int = if (reg in 0 until NUM_REGS) regs[reg] else 0

    fun setReg(reg: Int, value: Int) {
        val v = value and 0xFFFF
        if (reg in 0 until NUM_REGS && regs[reg] != v) {
            regs[reg] = v
            uiTick++
            if (isConnected) {
                val devReg = firmwareLayout.toDevice(reg)
                txQueue.offer(AxiosProtocol.buildParamWriteFrame(listOf(devReg to v)))
            }
        }
    }

    fun setGroupParam(group: Int, ch: Int, param: Int, value: Int) {
        val reg = AxiosProtocol.mapReg(group, ch, param)
        setReg(reg, value)
        // Propagate MAIN L/R stereo link when linked (except Pan 0x0B)
        if (param != 0x0B && group == 3 && (ch == 8 || ch == 9) && isOutputStereoLinked(8)) {
            val partner = ch xor 1
            setReg(AxiosProtocol.mapReg(3, partner, param), value)
        }
    }

    fun isOutputStereoLinked(outCh: Int): Boolean {
        val pair = (outCh / 2).coerceIn(0, 4)
        return ((getReg(0xBCC) ushr pair) and 1) != 0
    }

    fun setOutputStereoLinked(outCh: Int, linked: Boolean) {
        val pair = (outCh / 2).coerceIn(0, 4)
        val cur = getReg(0xBCC)
        val next = if (linked) cur or (1 shl pair) else cur and (1 shl pair).inv()
        setReg(0xBCC, next)
    }

    fun decodeFaderDb(raw: Int): Float {
        val mag = raw and 0x3FFF
        if (mag == 0) return -120f
        val db = (mag - 0x04B0) / 10f
        return db.coerceIn(-120f, 10f)
    }

    fun encodeFaderDb(db: Float, maxDb: Float = 10f): Int {
        if (db <= -119.5f) return 0
        val clamped = db.coerceIn(-120f, maxDb)
        return ((clamped * 10f).roundToInt() + 0x04B0).coerceAtLeast(0) and 0xFFFF
    }

    fun channelFaderDb(sel: SelectedChannel): Float {
        val (g, c) = sel.groupAndIndex()
        return decodeFaderDb(getReg(AxiosProtocol.mapReg(g, c, 0xEA)))
    }

    fun setChannelFaderDb(sel: SelectedChannel, db: Float) {
        val (g, c) = sel.groupAndIndex()
        setGroupParam(g, c, 0xEA, encodeFaderDb(db, 10f))
    }

    fun effectiveFaderDb(sel: SelectedChannel): Float = when (val sof = sendsOnFader) {
        is SendsOnFaderTarget.MainLR -> channelFaderDb(sel)
        is SendsOnFaderTarget.Aux -> if (sel is SelectedChannel.Input) inputAuxSendDb(sel.ch, sof.aux) else channelFaderDb(sel)
        is SendsOnFaderTarget.Fx -> if (sel is SelectedChannel.Input) inputFxSendDb(sel.ch, sof.fx) else channelFaderDb(sel)
    }

    fun setEffectiveFaderDb(sel: SelectedChannel, db: Float) {
        when (val sof = sendsOnFader) {
            is SendsOnFaderTarget.MainLR -> setChannelFaderDb(sel, db)
            is SendsOnFaderTarget.Aux -> if (sel is SelectedChannel.Input) setInputAuxSendDb(sel.ch, sof.aux, db) else setChannelFaderDb(sel, db)
            is SendsOnFaderTarget.Fx -> if (sel is SelectedChannel.Input) setInputFxSendDb(sel.ch, sof.fx, db) else setChannelFaderDb(sel, db)
        }
    }

    /**
     * Channel Mute verified against AX24E.25.11.01.001.exe (@ 0x00466A2C / 0x004679FA / 0x00441976):
     * - Groups 1 (Inputs), 2 (FX), 3 (AUX/MAIN L/R): param 0xEB INVERTED: 0 = MUTED, 1 = UNMUTED (ON)
     * - Group 4 (DCA): param 0xD9 (0xBA7/0xBCB + ch*9): 1 = MUTED (ON), 0 = UNMUTED (OFF)
     */
    fun channelMuted(sel: SelectedChannel): Boolean {
        val (g, c) = sel.groupAndIndex()
        if (g == 4) {
            return getReg(AxiosProtocol.mapReg(4, c, 0xD9)) != 0
        }
        return getReg(AxiosProtocol.mapReg(g, c, 0xEB)) == 0
    }

    fun setChannelMuted(sel: SelectedChannel, muted: Boolean) {
        val (g, c) = sel.groupAndIndex()
        if (g == 4) {
            setReg(AxiosProtocol.mapReg(4, c, 0xD9), if (muted) 1 else 0)
            return
        }
        val v = if (muted) 0 else 1
        setReg(AxiosProtocol.mapReg(g, c, 0xEB), v)
        if (g == 1 && c == 24) {
            setReg(AxiosProtocol.mapReg(1, 25, 0xEB), v)
        } else if (g == 3 && (c == 8 || c == 9) && isOutputStereoLinked(8)) {
            setReg(AxiosProtocol.mapReg(3, c xor 1, 0xEB), v)
        }
    }

    /**
     * Channel Solo / PFL verified against AX24E.25.11.01.001.exe (@ 0x00461390 & 0x004613B0):
     * param 0xEC (0xA00 MAIN L, 0xA6E MAIN R), active when (v & 0x3FFF) != 0 && (v & 0x4000) == 0.
     */
    fun channelSolo(sel: SelectedChannel): Boolean {
        val (g, c) = sel.groupAndIndex()
        if (g == 4) return false
        val reg = when {
            g == 3 && c == 8 -> 0xA00
            g == 3 && c == 9 -> 0xA6E
            else -> AxiosProtocol.mapReg(g, c, 0xEC)
        }
        val v = getReg(reg)
        return (v and 0x3FFF) != 0 && (v and 0x4000) == 0
    }

    fun setChannelSolo(sel: SelectedChannel, solo: Boolean) {
        val (g, c) = sel.groupAndIndex()
        if (g == 4) return
        val level = if (solo) 0x04B0 else 0
        when {
            g == 3 && c == 8 -> {
                setReg(0xA00, (getReg(0xA00) and 0xC000) or level)
                setReg(0xA01, 0)
            }
            g == 3 && c == 9 -> {
                setReg(0xA6E, (getReg(0xA6E) and 0xC000) or level)
                setReg(0xA6D, 0)
            }
            else -> {
                val reg = AxiosProtocol.mapReg(g, c, 0xEC)
                val next = (getReg(reg) and 0xC000) or level
                setReg(reg, next)
                setReg(reg + 1, next)
                if (g == 1 && c == 24) {
                    val reg2 = AxiosProtocol.mapReg(1, 25, 0xEC)
                    val next2 = (getReg(reg2) and 0xC000) or level
                    setReg(reg2, next2)
                    setReg(reg2 + 1, next2)
                }
            }
        }
    }

    fun clearAllSolos() {
        for (id in 0 until 42) {
            val sel = SelectedChannel.fromFlatId(id)
            if (channelSolo(sel)) setChannelSolo(sel, false)
        }
        toast("Todos os Solos foram limpos!")
    }

    fun inputPreampGainDb(ch: Int): Float =
        (getReg(AxiosProtocol.mapReg(1, ch.coerceIn(0, 24), 0x08)) and 0xFF).toFloat().coerceIn(0f, 60f)

    fun setInputPreampGainDb(ch: Int, gainDb: Float) {
        setGroupParam(1, ch.coerceIn(0, 24), 0x08, gainDb.roundToInt().coerceIn(0, 60))
    }

    /** +48V Phantom Power (param 0x01 = 0x41 + 0x3E*ch, verified in AX24E @ 0x0043CAEE) */
    fun inputPhantom48v(ch: Int): Boolean =
        ch in 0..23 && getReg(AxiosProtocol.mapReg(1, ch, 0x01)) != 0

    fun setInputPhantom48v(ch: Int, on: Boolean) {
        if (ch in 0..23) setReg(AxiosProtocol.mapReg(1, ch, 0x01), if (on) 1 else 0)
    }

    /** Input Phase Invert Ø (param 0x09 INVERTED in AX24E @ 0x0043967B: 0 = INVERTED/Ø, 1 = NORMAL) */
    fun inputPhaseInvert(ch: Int): Boolean =
        getReg(AxiosProtocol.mapReg(1, ch.coerceIn(0, 24), 0x09)) == 0

    fun setInputPhaseInvert(ch: Int, inv: Boolean) {
        setGroupParam(1, ch.coerceIn(0, 24), 0x09, if (inv) 0 else 1)
    }

    /** Input Delay in ms (0.0 .. 10.0 ms, param 0x1C, scale 10) */
    fun inputDelayMs(ch: Int): Float =
        (getReg(AxiosProtocol.mapReg(1, ch.coerceIn(0, 24), 0x1C)) / 10f).coerceIn(0f, 10f)

    fun setInputDelayMs(ch: Int, ms: Float) {
        setGroupParam(1, ch.coerceIn(0, 24), 0x1C, (ms.coerceIn(0f, 10f) * 10f).roundToInt())
    }

    /** Pan (-100..+100, param 0x0B stored as pan + 100 where 100 = Center) */
    fun inputPan(ch: Int): Int =
        (getReg(AxiosProtocol.mapReg(1, ch.coerceIn(0, 24), 0x0B)) - 100).coerceIn(-100, 100)

    fun setInputPan(ch: Int, pan: Int) {
        setGroupParam(1, ch.coerceIn(0, 24), 0x0B, (pan.coerceIn(-100, 100) + 100))
    }

    /** Input -> Main L/R bus assignment (params 0x15 & 0x16, verified in AX24E @ 0x004277BD / 0x004278C5) */
    fun inputToMainLr(ch: Int): Boolean {
        val c = ch.coerceIn(0, 24)
        val l = getReg(AxiosProtocol.mapReg(1, c, 0x15)) and 0x3FFF
        val r = getReg(AxiosProtocol.mapReg(1, c, 0x16)) and 0x3FFF
        return l != 0 || r != 0
    }

    fun setInputToMainLr(ch: Int, on: Boolean) {
        val c = ch.coerceIn(0, 24)
        val flL = getReg(AxiosProtocol.mapReg(1, c, 0x15)) and 0xE000
        val flR = getReg(AxiosProtocol.mapReg(1, c, 0x16)) and 0xE000
        val level = if (on) 0x04B0 else 0
        setGroupParam(1, c, 0x15, flL or level)
        setGroupParam(1, c, 0x16, flR or level)
        if (c == 24) {
            val flL2 = getReg(AxiosProtocol.mapReg(1, 25, 0x15)) and 0xE000
            val flR2 = getReg(AxiosProtocol.mapReg(1, 25, 0x16)) and 0xE000
            setGroupParam(1, 25, 0x15, flL2 or level)
            setGroupParam(1, 25, 0x16, flR2 or level)
        }
    }

    fun disconnectedLrCount(): Int =
        (0..24).count { !inputToMainLr(it) }

    fun routeAllInputsToMainLr() {
        for (ch in 0..24) {
            setInputToMainLr(ch, true)
        }
        setChannelMuted(SelectedChannel.Output(8), false)
        setChannelMuted(SelectedChannel.Output(9), false)
        toast("Todos os canais (CH01..24 + BT/DIGI) liberados para o MAIN L&R!")
    }

    fun inputAuxSendDb(ch: Int, aux: Int): Float =
        decodeFaderDb(getReg(AxiosProtocol.mapReg(1, ch.coerceIn(0, 24), 0x0D + aux.coerceIn(0, 7))))

    fun setInputAuxSendDb(ch: Int, aux: Int, db: Float) {
        val c = ch.coerceIn(0, 24)
        val param = 0x0D + aux.coerceIn(0, 7)
        val flags = getReg(AxiosProtocol.mapReg(1, c, param)) and 0xC000
        setGroupParam(1, c, param, (encodeFaderDb(db, 0f) and 0x3FFF) or flags)
    }

    fun inputFxSendDb(ch: Int, fx: Int): Float =
        decodeFaderDb(getReg(AxiosProtocol.mapReg(1, ch.coerceIn(0, 24), 0x19 + fx.coerceIn(0, 1))))

    fun setInputFxSendDb(ch: Int, fx: Int, db: Float) {
        val c = ch.coerceIn(0, 24)
        val param = 0x19 + fx.coerceIn(0, 1)
        val flags = getReg(AxiosProtocol.mapReg(1, c, param)) and 0xC000
        setGroupParam(1, c, param, (encodeFaderDb(db, 0f) and 0x3FFF) or flags)
    }

    // Gate & Compressor (verified exact offsets & types from AX24E.25.11.01.001.exe)
    fun gateEnabled(ch: Int): Boolean = getReg(AxiosProtocol.mapReg(1, ch.coerceIn(0, 24), 0x02)) != 0
    fun setGateEnabled(ch: Int, on: Boolean) = setGroupParam(1, ch.coerceIn(0, 24), 0x02, if (on) 1 else 0)
    fun gateThresholdDb(ch: Int): Float {
        val raw = getReg(AxiosProtocol.mapReg(1, ch.coerceIn(0, 24), 0x03)).toShort().toInt()
        val db = if (raw <= 0) raw.toFloat() else (-raw).toFloat()
        return db.coerceIn(-80f, 0f)
    }
    fun setGateThresholdDb(ch: Int, db: Float) {
        val raw = (-db.coerceIn(-80f, 0f).roundToInt()).coerceIn(0, 80)
        setGroupParam(1, ch.coerceIn(0, 24), 0x03, raw)
    }

    fun compEnabled(group: Int, ch: Int): Boolean {
        val p = if (group == 3) 0x44 else 0x1D
        return getReg(AxiosProtocol.mapReg(group, ch, p)) != 0
    }
    fun setCompEnabled(group: Int, ch: Int, on: Boolean) {
        val p = if (group == 3) 0x44 else 0x1D
        setGroupParam(group, ch, p, if (on) 1 else 0)
    }
    fun compThresholdDb(group: Int, ch: Int): Float {
        val p = if (group == 3) 0x48 else 0x21
        val raw = getReg(AxiosProtocol.mapReg(group, ch, p))
        val db = if ((raw and 0x8000) != 0) -((raw and 0x7FFF) / 10f) else -(raw / 10f)
        return db.coerceIn(-80f, 0f)
    }
    fun setCompThresholdDb(group: Int, ch: Int, db: Float) {
        val p = if (group == 3) 0x48 else 0x21
        val mag = (-db.coerceIn(-80f, 0f) * 10f).roundToInt()
        val raw = if (mag > 0) (mag or 0x8000) else 0
        setGroupParam(group, ch, p, raw)
    }
    fun compRatio(group: Int, ch: Int): Float {
        val p = if (group == 3) 0x45 else 0x1E
        val raw = getReg(AxiosProtocol.mapReg(group, ch, p)).toFloat()
        val ratio = if (raw >= 50f) raw / 100f else raw / 10f
        return ratio.coerceIn(1f, 20f)
    }
    fun setCompRatio(group: Int, ch: Int, ratio: Float) {
        val p = if (group == 3) 0x45 else 0x1E
        val scale = if (firmwareLayout == FirmwareLayout.AX24E_2511) 100f else 10f
        setGroupParam(group, ch, p, (ratio.coerceIn(1f, 20f) * scale).roundToInt())
    }

    // EQ & Quick HPF 100Hz
    private fun eqOffsets(group: Int): IntArray = when (group) {
        2 -> intArrayOf(0xB9, 0xBA, 0xBB, 0xBC, 0xBD, 0xBF)
        3 -> intArrayOf(0x4D, 0x4E, 0x4F, 0x50, 0x51, 0x53)
        else -> intArrayOf(0x24, 0x25, 0x26, 0x27, 0x28, 0x2A)
    }

    fun decodeFreqHz(raw: Int): Float =
        if ((raw and 0x8000) != 0) ((raw and 0x7FFF) / 10f) else raw.toFloat().coerceAtLeast(20f)

    fun encodeFreqHz(hz: Float): Int {
        val f = hz.coerceIn(19.6f, 20000f)
        return if (f < 100f) ((f * 10f).roundToInt() or 0x8000) else f.roundToInt()
    }

    fun eqEnabled(group: Int, ch: Int): Boolean = getReg(AxiosProtocol.mapReg(group, ch, eqOffsets(group)[0])) != 0
    fun setEqEnabled(group: Int, ch: Int, on: Boolean) = setGroupParam(group, ch, eqOffsets(group)[0], if (on) 1 else 0)

    fun hpfEnabled(group: Int, ch: Int): Boolean = getReg(AxiosProtocol.mapReg(group, ch, eqOffsets(group)[1])) != 0
    fun setHpfEnabled(group: Int, ch: Int, on: Boolean) = setGroupParam(group, ch, eqOffsets(group)[1], if (on) 1 else 0)

    fun hpfFreqHz(group: Int, ch: Int): Float = decodeFreqHz(getReg(AxiosProtocol.mapReg(group, ch, eqOffsets(group)[3]))).coerceIn(20f, 20000f)
    fun setHpfFreqHz(group: Int, ch: Int, hz: Float) = setGroupParam(group, ch, eqOffsets(group)[3], encodeFreqHz(hz))

    fun peqBandGainDb(group: Int, ch: Int, band: Int): Float {
        val base = eqOffsets(group)[5] + band * 4
        val raw = getReg(AxiosProtocol.mapReg(group, ch, base + 2))
        return ((raw - 500) / 10f).coerceIn(-12f, 12f)
    }
    fun setPeqBandGainDb(group: Int, ch: Int, band: Int, db: Float) {
        val base = eqOffsets(group)[5] + band * 4
        val regVal = ((db.coerceIn(-12f, 12f) * 10f).roundToInt() + 500).coerceAtLeast(0)
        setGroupParam(group, ch, base + 2, regVal)
    }
    fun peqBandFreqHz(group: Int, ch: Int, band: Int): Float {
        val base = eqOffsets(group)[5] + band * 4
        return decodeFreqHz(getReg(AxiosProtocol.mapReg(group, ch, base + 1))).coerceIn(20f, 20000f)
    }
    fun setPeqBandFreqHz(group: Int, ch: Int, band: Int, hz: Float) {
        val base = eqOffsets(group)[5] + band * 4
        setGroupParam(group, ch, base + 1, encodeFreqHz(hz))
    }

    fun toggleQuickHpf100Hz(sel: SelectedChannel) {
        val (g, c) = sel.groupAndIndex()
        if (g == 4) return
        if (eqEnabled(g, c) && hpfEnabled(g, c) && hpfFreqHz(g, c) >= 45f) {
            setHpfEnabled(g, c, false)
            setHpfFreqHz(g, c, 20f)
            toast("HPF desligado em ${channelName(sel)}")
        } else {
            setEqEnabled(g, c, true)
            setHpfEnabled(g, c, true)
            setGroupParam(g, c, eqOffsets(g)[2], 14) // 18dB Butterworth
            setHpfFreqHz(g, c, 100f)
            toast("HPF 100 Hz (18dB) ativo em ${channelName(sel)}!")
        }
    }

    fun applyChannelPreset(sel: SelectedChannel, p: ChannelPreset) {
        if (sel !is SelectedChannel.Input || sel.ch >= 24) return
        val ch = sel.ch
        setInputPreampGainDb(ch, p.gainDb)
        setEqEnabled(1, ch, true)
        setHpfEnabled(1, ch, true)
        setHpfFreqHz(1, ch, p.hpfHz)
        setGateEnabled(ch, true)
        setGateThresholdDb(ch, p.gateTh)
        setCompEnabled(1, ch, true)
        setCompThresholdDb(1, ch, p.compTh)
        setCompRatio(1, ch, p.compRatio)
        setInputToMainLr(ch, true)
        toast("Preset '${p.label}' aplicado em ${channelName(sel)}!")
    }

    // Safe Unplug ("Desplugar") individual & all
    fun toggleSafeUnplug(ch: Int) {
        if (ch !in 0..24) return
        val sel = SelectedChannel.Input(ch)
        val next = !safeUnplugged[ch]
        setSafeUnplugInternal(ch, next)
        if (next) {
            toast("Desplugar ATIVO em ${channelName(sel)} (-120 dB)")
        } else {
            toast("Cabo reconectado em ${channelName(sel)} (${String.format("%+.1f dB", savedFaderBeforeUnplug[ch])})")
        }
    }

    private fun setSafeUnplugInternal(ch: Int, active: Boolean) {
        if (ch !in 0..24 || safeUnplugged[ch] == active) return
        val sel = SelectedChannel.Input(ch)
        if (active) {
            savedFaderBeforeUnplug[ch] = channelFaderDb(sel)
            safeUnplugged[ch] = true
            setChannelMuted(sel, true)
            setChannelFaderDb(sel, -120f)
        } else {
            safeUnplugged[ch] = false
            setChannelFaderDb(sel, savedFaderBeforeUnplug[ch])
            setChannelMuted(sel, false)
        }
    }

    fun allSafeUnplugged(): Boolean = (0 until 24).all { safeUnplugged[it] }

    fun toggleSafeUnplugAll() {
        val activate = !allSafeUnplugged()
        for (ch in 0 until 24) setSafeUnplugInternal(ch, activate)
        toast(
            if (activate) "Desplugar TODOS ATIVO (CH1-24 em -120 dB)!"
            else "Desplugar TODOS DESATIVADO (Volumes restaurados)!"
        )
    }

    // Bluetooth & Media Source (opcode 0x72)
    fun setMediaSource(sourceId: Int) {
        mediaSourceId = sourceId
        if (isConnected) {
            txQueue.offer(AxiosProtocol.buildMediaSourceFrame(sourceId))
        }
        val lbl = when (sourceId) {
            0 -> "USB Music"
            1 -> "Bluetooth"
            2 -> "Optical"
            else -> "Coaxial"
        }
        toast("Fonte Digital: $lbl (canal DIGI)")
    }

    fun enableBluetoothPlaybackQuick() {
        setMediaSource(1)
        setInputToMainLr(24, true)
        setChannelMuted(SelectedChannel.Input(24), false)
        setChannelFaderDb(SelectedChannel.Input(24), 0f)
        toast("Bluetooth ativo e canal DIGI aberto em 0.0 dB!")
    }

    // Scenes
    fun recallScene(idx: Int) {
        if (idx !in 0..15) return
        scenes[idx].copyInto(regs)
        uiTick++
        if (isConnected) {
            txQueue.offer(AxiosProtocol.buildSceneOpFrame(idx + 1, false))
            readAllRegistersFromHardware()
        }
        toast("Cena ${idx + 1} (${sceneNames[idx]}) carregada!")
    }

    fun saveScene(idx: Int) {
        if (idx !in 0..15) return
        regs.copyInto(scenes[idx])
        if (isConnected) {
            txQueue.offer(AxiosProtocol.buildSceneOpFrame(idx + 1, true))
        }
        saveUserPrefs()
        toast("Cena ${idx + 1} (${sceneNames[idx]}) salva!")
    }

    fun channelName(sel: SelectedChannel): String = channelNames.getOrElse(sel.flatId()) { sel.shortBadge() }

    fun setChannelName(sel: SelectedChannel, name: String) {
        val id = sel.flatId()
        if (id in 0 until 60) {
            val clean = name.trim().ifEmpty { sel.shortBadge() }
            channelNames[id] = clean
            if (isConnected) {
                val nb = clean.toByteArray(Charsets.UTF_8)
                val payload = ByteArray(1 + nb.size)
                payload[0] = id.toByte()
                System.arraycopy(nb, 0, payload, 1, nb.size)
                txQueue.offer(AxiosProtocol.buildFrame(0x2F, payload))
            }
            saveUserPrefs()
        }
    }

    // Network Discovery & Connection
    fun scanNetworkDevices() {
        if (isScanning) return
        isScanning = true
        localPhoneIp = AxiosProtocol.detectLocalIpv4() ?: "Sem Wi-Fi/LAN"
        viewModelScope.launch(Dispatchers.IO) {
            val found = AxiosProtocol.probeUdpDevices()
            withContext(Dispatchers.Main) {
                discoveredDevices.clear()
                discoveredDevices.addAll(found)
                if (found.isNotEmpty()) {
                    connectIp = found.first().ip
                    FirmwareLayout.detect(found.first().name, found.first().fwVersion)?.let {
                        firmwareLayout = it
                    }
                }
                isScanning = false
            }
        }
    }

    fun onAppResumed() {
        val proto = desiredProto ?: return
        if (!netRunning.get() || !isConnected || netJob?.isActive != true) {
            if (proto == "TCP") connectTcp(connectIp) else connectUdp(connectIp)
        } else {
            acquireWifiLock()
            txQueue.offer(AxiosProtocol.buildFrame(0x00, byteArrayOf(0x00)))
            txQueue.offer(AxiosProtocol.buildParamReadFrame(vuMeterRegs))
            txQueue.offer(AxiosProtocol.buildParamReadFrame(listOf(0)))
        }
    }

    fun connectUdp(ip: String) {
        val targetIp = ip.trim()
        stopNetworkWorker(clearDesired = false)
        connectIp = targetIp
        desiredProto = "UDP"
        saveUserPrefs()
        val myToken = connToken.incrementAndGet()
        netRunning.set(true)
        isConnected = true
        lastHwRxMs.set(System.currentTimeMillis())
        acquireWifiLock()
        for (i in 0 until channelMetersDb.size) channelMetersDb[i] = -90f
        connectionStatus = "UDP $connectIp:$UDP_REMOTE_PORT"

        netJob = viewModelScope.launch(Dispatchers.IO) {
            val discoverPkt = AxiosProtocol.buildFrame(0x00, byteArrayOf(0x00))
            val keepAlive = AxiosProtocol.buildParamReadFrame(listOf(0))
            val vuPoll = AxiosProtocol.buildParamReadFrame(vuMeterRegs)
            val rtaPoll = AxiosProtocol.buildFrame(0x46, byteArrayOf(0x00, 0x1F))
            val buf = ByteArray(2048)
            var needsInitialSync = true

            while (netRunning.get() && connToken.get() == myToken) {
                try {
                    val addr = InetAddress.getByName(connectIp)
                    DatagramSocket().use { sock ->
                        sock.broadcast = true
                        sock.soTimeout = 25
                        sock.send(DatagramPacket(discoverPkt, discoverPkt.size, addr, UDP_REMOTE_PORT))
                        sock.send(DatagramPacket(keepAlive, keepAlive.size, addr, UDP_REMOTE_PORT))
                        sock.send(DatagramPacket(vuPoll, vuPoll.size, addr, UDP_REMOTE_PORT))
                        if (needsInitialSync) {
                            readAllRegistersFromHardware()
                            needsInitialSync = false
                        }

                        var ticks = 0
                        var reconnectingBanner = false
                        while (netRunning.get() && connToken.get() == myToken) {
                            var sentNow = 0
                            while (sentNow < 4) {
                                val pkt = txQueue.poll() ?: break
                                sock.send(DatagramPacket(pkt, pkt.size, addr, UDP_REMOTE_PORT))
                                sentNow++
                            }
                            sock.soTimeout = if (sentNow > 0) 8 else 18
                            var gotAny = false
                            for (i in 0 until 24) {
                                try {
                                    val rx = DatagramPacket(buf, buf.size)
                                    sock.receive(rx)
                                    handleIncomingPacket(rx.data, rx.length)
                                    gotAny = true
                                    sock.soTimeout = 2
                                } catch (_: Exception) {
                                    break
                                }
                            }
                            if (gotAny && reconnectingBanner) {
                                reconnectingBanner = false
                                withContext(Dispatchers.Main) {
                                    if (connToken.get() == myToken) {
                                        connectionStatus = "UDP $connectIp:$UDP_REMOTE_PORT"
                                    }
                                }
                            }
                            ticks++
                            // Poll real per-channel hardware VU meters (0x02..0x0E, 0x29..0x2F) every ~90ms
                            if (sentNow == 0 && ticks % 5 == 0) {
                                sock.send(DatagramPacket(vuPoll, vuPoll.size, addr, UDP_REMOTE_PORT))
                            }
                            // Poll 31-band RTA (0x46) every ~270ms
                            if (sentNow == 0 && ticks % 15 == 0) {
                                sock.send(DatagramPacket(rtaPoll, rtaPoll.size, addr, UDP_REMOTE_PORT))
                            }
                            // Heartbeat + stale link recovery every ~1.8s
                            if (ticks % 100 == 0) {
                                sock.send(DatagramPacket(keepAlive, keepAlive.size, addr, UDP_REMOTE_PORT))
                                val silentMs = System.currentTimeMillis() - lastHwRxMs.get()
                                if (silentMs > 4500L) {
                                    sock.send(DatagramPacket(discoverPkt, discoverPkt.size, addr, UDP_REMOTE_PORT))
                                    if (!reconnectingBanner) {
                                        reconnectingBanner = true
                                        withContext(Dispatchers.Main) {
                                            if (connToken.get() == myToken) {
                                                connectionStatus = "Reconectando UDP $connectIp..."
                                            }
                                        }
                                    }
                                    if (silentMs > 9000L) {
                                        // Recreate UDP socket in case Android killed network route during sleep
                                        break
                                    }
                                }
                            }
                        }
                    }
                } catch (_: Exception) {
                    if (netRunning.get() && connToken.get() == myToken) {
                        withContext(Dispatchers.Main) {
                            if (connToken.get() == myToken) {
                                connectionStatus = "Reconectando UDP $connectIp..."
                            }
                        }
                        delay(800)
                    }
                }
            }
        }
    }

    fun connectTcp(ip: String) {
        val targetIp = ip.trim()
        stopNetworkWorker(clearDesired = false)
        connectIp = targetIp
        desiredProto = "TCP"
        saveUserPrefs()
        val myToken = connToken.incrementAndGet()
        netRunning.set(true)
        isConnected = true
        lastHwRxMs.set(System.currentTimeMillis())
        acquireWifiLock()
        for (i in 0 until channelMetersDb.size) channelMetersDb[i] = -90f
        connectionStatus = "TCP $connectIp:$TCP_REMOTE_PORT"

        netJob = viewModelScope.launch(Dispatchers.IO) {
            val keepAlive = AxiosProtocol.buildKvFrame("DEVNAME=\n", false)
            val vuPoll = AxiosProtocol.buildParamReadFrame(vuMeterRegs)
            val rtaPoll = AxiosProtocol.buildFrame(0x46, byteArrayOf(0x00, 0x1F))
            val acc = ArrayList<Byte>(4096)

            while (netRunning.get() && connToken.get() == myToken) {
                try {
                    Socket().use { s ->
                        s.connect(InetSocketAddress(connectIp, TCP_REMOTE_PORT), 2000)
                        s.soTimeout = 25
                        s.tcpNoDelay = true
                        s.keepAlive = true
                        val inp = s.getInputStream()
                        val out = s.getOutputStream()
                        val buf = ByteArray(1024)
                        acc.clear()
                        var ticks = 0

                        withContext(Dispatchers.Main) {
                            if (connToken.get() == myToken) {
                                connectionStatus = "TCP $connectIp:$TCP_REMOTE_PORT"
                            }
                        }
                        out.write(keepAlive)
                        out.write(vuPoll)
                        readAllRegistersFromHardware()

                        while (netRunning.get() && connToken.get() == myToken) {
                            var sentNow = 0
                            while (sentNow < 8) {
                                val pkt = txQueue.poll() ?: break
                                out.write(pkt)
                                sentNow++
                            }
                            try {
                                val n = inp.read(buf)
                                if (n > 0) {
                                    for (i in 0 until n) acc.add(buf[i])
                                    while (true) {
                                        val pos = acc.indexOfFirst { (it.toInt() and 0xFF) == 0x80 }
                                        if (pos < 0) {
                                            acc.clear()
                                            break
                                        }
                                        if (pos > 0) acc.subList(0, pos).clear()
                                        if (acc.size < 5) break
                                        val b1 = acc[1].toInt() and 0xFF
                                        val b2 = acc[2].toInt() and 0xFF
                                        val crcOff = if (b1 == 0xFF && b2 == 0xFF) 0x106 else b1
                                        val total = crcOff + 2
                                        if (crcOff < 3 || total > 512) {
                                            acc.removeAt(0)
                                            continue
                                        }
                                        if (acc.size < total) break
                                        val frame = ByteArray(total) { idx -> acc[idx] }
                                        if (AxiosProtocol.parseFrame(frame, total) != null) {
                                            acc.subList(0, total).clear()
                                            handleIncomingPacket(frame, total)
                                        } else {
                                            acc.removeAt(0)
                                        }
                                    }
                                } else if (n < 0) {
                                    break
                                }
                            } catch (_: java.net.SocketTimeoutException) {
                            }
                            ticks++
                            if (sentNow == 0 && ticks % 4 == 0) out.write(vuPoll)
                            if (sentNow == 0 && ticks % 12 == 0) out.write(rtaPoll)
                            if (ticks % 80 == 0) out.write(keepAlive)
                        }
                    }
                } catch (_: Exception) {
                    if (netRunning.get() && connToken.get() == myToken) {
                        withContext(Dispatchers.Main) {
                            if (connToken.get() == myToken) {
                                connectionStatus = "Reconectando TCP $connectIp..."
                            }
                        }
                        delay(1000)
                    }
                }
            }
        }
    }

    private fun stopNetworkWorker(clearDesired: Boolean) {
        if (clearDesired) desiredProto = null
        netRunning.set(false)
        connToken.incrementAndGet()
        netJob?.cancel()
        netJob = null
        txQueue.clear()
        releaseWifiLock()
    }

    fun disconnect() {
        stopNetworkWorker(clearDesired = true)
        isConnected = false
        connectionStatus = "MODO DEMO (Offline)"
    }

    fun readAllRegistersFromHardware() {
        if (!isConnected) return
        val batch = ArrayList<Int>(32)
        for (idx in 0 until NUM_REGS) {
            batch.add(firmwareLayout.toDevice(idx))
            if (batch.size == 32) {
                txQueue.offer(AxiosProtocol.buildParamReadFrame(batch))
                batch.clear()
            }
        }
        if (batch.isNotEmpty()) {
            txQueue.offer(AxiosProtocol.buildParamReadFrame(batch))
        }
        for (ch in 0 until 60) {
            txQueue.offer(AxiosProtocol.buildFrame(0x2E, byteArrayOf(ch.toByte())))
        }
        for (slot in 1..16) {
            txQueue.offer(AxiosProtocol.buildFrame(0x04, byteArrayOf(slot.toByte())))
        }
    }

    fun syncAllRegistersToHardware() {
        if (!isConnected) {
            toast("Conecte-se à mesa via UDP ou TCP primeiro!")
            return
        }
        val batch = ArrayList<Pair<Int, Int>>(24)
        for (idx in 0 until NUM_REGS) {
            batch.add(firmwareLayout.toDevice(idx) to regs[idx])
            if (batch.size == 24) {
                txQueue.offer(AxiosProtocol.buildParamWriteFrame(batch))
                batch.clear()
            }
        }
        if (batch.isNotEmpty()) {
            txQueue.offer(AxiosProtocol.buildParamWriteFrame(batch))
        }
        toast("Cena completa enviada para a Axios 24!")
    }

    private fun applyVuMeterUpdate(flatId: Int, signedByte: Int) {
        if (flatId !in 0 until channelMetersDb.size) return
        val db = signedByte.toByte().toFloat().coerceIn(-90f, 3f)
        val prev = channelMetersDb[flatId]
        val smoothed = if (db >= prev) db else (prev - 4.5f).coerceAtLeast(db)
        channelMetersDb[flatId] = smoothed
        if (db >= -0.5f) channelClipHold[flatId] = true
    }

    private suspend fun handleIncomingPacket(data: ByteArray, length: Int) {
        val parsed = AxiosProtocol.parseFrame(data, length) ?: return
        lastHwRxMs.set(System.currentTimeMillis())
        val (cmd, payload) = parsed
        if (cmd == 0x03 || cmd == 0x06 || cmd == 0x25) {
            val updates = ArrayList<Pair<Int, Int>>()
            val vuUpdates = ArrayList<Pair<Int, Int>>()
            var i = 0
            while (i + 4 <= payload.size) {
                val r = ((payload[i].toInt() and 0xFF) shl 8) or (payload[i + 1].toInt() and 0xFF)
                val v = ((payload[i + 2].toInt() and 0xFF) shl 8) or (payload[i + 3].toInt() and 0xFF)
                if (r in 0x02..0x0E || r in 0x29..0x2F) {
                    vuUpdates.add(r to v)
                } else {
                    updates.add(r to v)
                }
                i += 4
            }
            if (vuUpdates.isNotEmpty()) {
                val hasNonZero = vuUpdates.any { it.second != 0 }
                if (hasNonZero) {
                    lastVuRxMs.set(System.currentTimeMillis())
                    withContext(Dispatchers.Main) {
                        for ((r, v) in vuUpdates) {
                            if (v == 0) continue
                            val lo = v and 0xFF
                            val hi = (v ushr 8) and 0xFF
                            when (r) {
                                in 0x02..0x0E -> {
                                    val ch0 = (r - 0x02) * 2
                                    applyVuMeterUpdate(ch0, lo)
                                    if (ch0 + 1 <= 25) applyVuMeterUpdate(ch0 + 1, hi)
                                }
                                0x29, 0x2A -> {
                                    applyVuMeterUpdate(26 + (r - 0x29), lo)
                                }
                                in 0x2B..0x2F -> {
                                    val out0 = (r - 0x2B) * 2
                                    applyVuMeterUpdate(28 + out0, lo)
                                    if (out0 + 1 <= 9) applyVuMeterUpdate(28 + out0 + 1, hi)
                                }
                            }
                        }
                    }
                }
            }
            if (updates.isNotEmpty()) {
                withContext(Dispatchers.Main) {
                    if (updates.any { it.first >= NUM_REGS }) {
                        firmwareLayout = FirmwareLayout.AX24E_2511
                    }
                    for ((devReg, v) in updates) {
                        val r = firmwareLayout.fromDevice(devReg)
                        if (r != null) regs[r] = v
                    }
                    uiTick++
                }
            }
        } else if (cmd == 0x2E && payload.isNotEmpty()) {
            val ch = payload[0].toInt() and 0xFF
            val nm = payload.copyOfRange(1, payload.size).toString(Charsets.UTF_8).trim('\u0000', ' ')
            if (ch in 0 until channelNames.size && nm.isNotEmpty()) {
                withContext(Dispatchers.Main) {
                    channelNames[ch] = nm
                    uiTick++
                }
            }
        } else if (cmd == 0x04 && payload.isNotEmpty()) {
            val slot = payload[0].toInt() and 0xFF
            val nm = payload.copyOfRange(1, payload.size).toString(Charsets.UTF_8).trim('\u0000', ' ')
            if (slot in 1..16 && nm.isNotEmpty()) {
                withContext(Dispatchers.Main) {
                    sceneNames[slot - 1] = nm
                    uiTick++
                }
            }
        } else if (cmd == 0x46 && payload.size >= 3) {
            // Fallback for MAIN L/R meter when only 0x46 RTA is available (e.g. emulator)
            if (System.currentTimeMillis() - lastVuRxMs.get() > 500L) {
                val peak = payload.drop(2).maxOfOrNull { it.toInt() and 0xFF } ?: 0
                val masterDb = if (peak <= 1) -90f else (-60f + (peak / 100f) * 60f).coerceIn(-90f, 2f)
                withContext(Dispatchers.Main) {
                    channelMetersDb[36] = masterDb
                    channelMetersDb[37] = masterDb
                }
            }
        } else if (cmd != 0x39 && cmd != 0x00) {
            val preview = payload.take(12).joinToString(" ") { String.format("%02X", it.toInt() and 0xFF) }
            val info = "Opcode 0x${String.format("%02X", cmd)} (${payload.size}B): $preview"
            withContext(Dispatchers.Main) {
                lastUnknownPacketInfo = info
            }
        }
    }

    private fun startMeterSimulationLoop() {
        viewModelScope.launch {
            var t = 0f
            while (true) {
                delay(50)
                t += 0.05f
                if (!isConnected) {
                    for (i in 0 until 42) {
                        val sel = SelectedChannel.fromFlatId(i)
                        if (channelMuted(sel)) {
                            channelMetersDb[i] = -90f
                        } else {
                            val fader = channelFaderDb(sel)
                            val wave = sin(t * (2.2f + (i % 5) * 0.35f) + i * 0.7f) * 10f
                            val lvl = (-18f + wave + fader * 0.65f).coerceIn(-85f, 2f)
                            channelMetersDb[i] = lvl
                            if (lvl >= -0.5f) channelClipHold[i] = true
                        }
                    }
                } else if (System.currentTimeMillis() - lastVuRxMs.get() > 350L) {
                    for (i in 0 until 42) {
                        val cur = channelMetersDb[i]
                        if (cur > -90f) {
                            channelMetersDb[i] = (cur - 3.0f).coerceAtLeast(-90f)
                        }
                    }
                }
            }
        }
    }

    fun saveUserPrefs() {
        val ed = prefs.edit()
        ed.putString("connect_ip", connectIp)
        ed.putString("fw_layout", firmwareLayout.name)
        for (i in 0 until 60) ed.putString("ch_name_$i", channelNames[i])
        ed.apply()
    }

    private fun loadUserPrefs() {
        connectIp = prefs.getString("connect_ip", "172.14.10.1") ?: "172.14.10.1"
        val fw = prefs.getString("fw_layout", FirmwareLayout.AX24E_2511.name)
        firmwareLayout = if (fw == FirmwareLayout.LEGACY.name) FirmwareLayout.LEGACY else FirmwareLayout.AX24E_2511
        for (i in 0 until 60) {
            prefs.getString("ch_name_$i", null)?.let { if (it.isNotBlank()) channelNames[i] = it }
        }
    }
}
