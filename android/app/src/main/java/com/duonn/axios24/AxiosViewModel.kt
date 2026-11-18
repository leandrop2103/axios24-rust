package com.duonn.axios24

import android.app.Application
import android.content.Context
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.mutableStateListOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import androidx.lifecycle.AndroidViewModel
import androidx.lifecycle.viewModelScope
import kotlinx.coroutines.Dispatchers
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

    init {
        loadUserPrefs()
        startMeterSimulationLoop()
        scanNetworkDevices()
    }

    private fun defaultRegValue(idx: Int): Int {
        // Initialize default 0dB faders on MAIN L/R (0xA00/0xA6D), stereo link on MAIN L/R (0xBCC bit 4), and reasonable defaults
        return when (idx) {
            0xBCC -> 1 shl 4
            0xA00, 0xA6D -> 0x04B0 // 0.0 dB on MAIN L & R
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
        val v = raw.toShort().toInt()
        val db = (v - 0x04B0) / 10f
        return db.coerceIn(-120f, 10f)
    }

    fun encodeFaderDb(db: Float, maxDb: Float = 10f): Int {
        val clamped = db.coerceIn(-120f, maxDb)
        return ((clamped * 10f).roundToInt() + 0x04B0) and 0xFFFF
    }

    fun channelFaderDb(sel: SelectedChannel): Float {
        val (g, c) = sel.groupAndIndex()
        return decodeFaderDb(getReg(AxiosProtocol.mapReg(g, c, 0xEC)))
    }

    fun setChannelFaderDb(sel: SelectedChannel, db: Float) {
        val (g, c) = sel.groupAndIndex()
        setGroupParam(g, c, 0xEC, encodeFaderDb(db, 10f))
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

    fun channelMuted(sel: SelectedChannel): Boolean {
        val (g, c) = sel.groupAndIndex()
        return getReg(AxiosProtocol.mapReg(g, c, 0xEA)) != 0
    }

    fun setChannelMuted(sel: SelectedChannel, muted: Boolean) {
        val (g, c) = sel.groupAndIndex()
        setGroupParam(g, c, 0xEA, if (muted) 1 else 0)
    }

    fun channelSolo(sel: SelectedChannel): Boolean {
        val (g, c) = sel.groupAndIndex()
        return getReg(AxiosProtocol.mapReg(g, c, 0xEB)) != 0
    }

    fun setChannelSolo(sel: SelectedChannel, solo: Boolean) {
        val (g, c) = sel.groupAndIndex()
        setGroupParam(g, c, 0xEB, if (solo) 1 else 0)
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

    fun inputPhantom48v(ch: Int): Boolean =
        ch in 0..23 && getReg(0xADF + ch) != 0

    fun setInputPhantom48v(ch: Int, on: Boolean) {
        if (ch in 0..23) setReg(0xADF + ch, if (on) 1 else 0)
    }

    fun inputPhaseInvert(ch: Int): Boolean =
        getReg(AxiosProtocol.mapReg(1, ch.coerceIn(0, 24), 0x09)) != 0

    fun setInputPhaseInvert(ch: Int, inv: Boolean) {
        setGroupParam(1, ch.coerceIn(0, 24), 0x09, if (inv) 1 else 0)
    }

    fun inputDelayMs(ch: Int): Float =
        (getReg(AxiosProtocol.mapReg(1, ch.coerceIn(0, 24), 0x1C)) / 10f).coerceIn(0f, 40f)

    fun setInputDelayMs(ch: Int, ms: Float) {
        setGroupParam(1, ch.coerceIn(0, 24), 0x1C, (ms.coerceIn(0f, 40f) * 10f).roundToInt())
    }

    fun inputPan(ch: Int): Int =
        getReg(AxiosProtocol.mapReg(1, ch.coerceIn(0, 24), 0x0B)).toShort().toInt().coerceIn(-100, 100)

    fun setInputPan(ch: Int, pan: Int) {
        setGroupParam(1, ch.coerceIn(0, 24), 0x0B, pan.coerceIn(-100, 100) and 0xFFFF)
    }

    fun inputToMainLr(ch: Int): Boolean =
        getReg(AxiosProtocol.mapReg(1, ch.coerceIn(0, 24), 0x17)) != 0

    fun setInputToMainLr(ch: Int, on: Boolean) {
        setGroupParam(1, ch.coerceIn(0, 24), 0x17, if (on) 1 else 0)
    }

    fun inputAuxSendDb(ch: Int, aux: Int): Float =
        decodeFaderDb(getReg(AxiosProtocol.mapReg(1, ch.coerceIn(0, 24), 0x0D + aux.coerceIn(0, 7))))

    fun setInputAuxSendDb(ch: Int, aux: Int, db: Float) {
        setGroupParam(1, ch.coerceIn(0, 24), 0x0D + aux.coerceIn(0, 7), encodeFaderDb(db, 10f))
    }

    fun inputFxSendDb(ch: Int, fx: Int): Float =
        decodeFaderDb(getReg(AxiosProtocol.mapReg(1, ch.coerceIn(0, 24), 0x19 + fx.coerceIn(0, 1))))

    fun setInputFxSendDb(ch: Int, fx: Int, db: Float) {
        setGroupParam(1, ch.coerceIn(0, 24), 0x19 + fx.coerceIn(0, 1), encodeFaderDb(db, 0f))
    }

    // Gate & Compressor
    fun gateEnabled(ch: Int): Boolean = getReg(AxiosProtocol.mapReg(1, ch.coerceIn(0, 24), 0x02)) != 0
    fun setGateEnabled(ch: Int, on: Boolean) = setGroupParam(1, ch.coerceIn(0, 24), 0x02, if (on) 1 else 0)
    fun gateThresholdDb(ch: Int): Float = ((getReg(AxiosProtocol.mapReg(1, ch.coerceIn(0, 24), 0x03)).toShort().toInt() - 0x04B0) / 10f).coerceIn(-80f, 0f)
    fun setGateThresholdDb(ch: Int, db: Float) = setGroupParam(1, ch.coerceIn(0, 24), 0x03, ((db.coerceIn(-80f, 0f) * 10f).roundToInt() + 0x04B0) and 0xFFFF)

    fun compEnabled(group: Int, ch: Int): Boolean {
        val p = if (group == 3) 0x46 else 0x1B
        return getReg(AxiosProtocol.mapReg(group, ch, p)) != 0
    }
    fun setCompEnabled(group: Int, ch: Int, on: Boolean) {
        val p = if (group == 3) 0x46 else 0x1B
        setGroupParam(group, ch, p, if (on) 1 else 0)
    }
    fun compThresholdDb(group: Int, ch: Int): Float {
        val p = if (group == 3) 0x47 else 0x1D
        return ((getReg(AxiosProtocol.mapReg(group, ch, p)).toShort().toInt() - 0x04B0) / 10f).coerceIn(-60f, 0f)
    }
    fun setCompThresholdDb(group: Int, ch: Int, db: Float) {
        val p = if (group == 3) 0x47 else 0x1D
        setGroupParam(group, ch, p, ((db.coerceIn(-60f, 0f) * 10f).roundToInt() + 0x04B0) and 0xFFFF)
    }
    fun compRatio(group: Int, ch: Int): Float {
        val p = if (group == 3) 0x48 else 0x1E
        return (getReg(AxiosProtocol.mapReg(group, ch, p)) / 10f).coerceIn(1f, 20f)
    }
    fun setCompRatio(group: Int, ch: Int, ratio: Float) {
        val p = if (group == 3) 0x48 else 0x1E
        setGroupParam(group, ch, p, (ratio.coerceIn(1f, 20f) * 10f).roundToInt())
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
        val raw = getReg(AxiosProtocol.mapReg(group, ch, base + 2)).toShort().toInt()
        return ((raw - 0x04B0) / 10f).coerceIn(-15f, 15f)
    }
    fun setPeqBandGainDb(group: Int, ch: Int, band: Int, db: Float) {
        val base = eqOffsets(group)[5] + band * 4
        setGroupParam(group, ch, base + 2, ((db.coerceIn(-15f, 15f) * 10f).roundToInt() + 0x04B0) and 0xFFFF)
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
            channelNames[id] = name.trim().ifEmpty { sel.shortBadge() }
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

    fun connectUdp(ip: String) {
        disconnect()
        connectIp = ip.trim()
        saveUserPrefs()
        netRunning.set(true)
        isConnected = true
        connectionStatus = "UDP $connectIp:$UDP_REMOTE_PORT"

        viewModelScope.launch(Dispatchers.IO) {
            try {
                DatagramSocket().use { sock ->
                    sock.soTimeout = 30
                    val addr = InetAddress.getByName(connectIp)
                    val keepAlive = AxiosProtocol.buildKvFrame("DEVNAME=\nFW_VER=\n", false)
                    val buf = ByteArray(512)
                    var ticks = 0

                    // Automatically pull current scene from mixer upon connection
                    readAllRegistersFromHardware()

                    while (netRunning.get()) {
                        while (true) {
                            val pkt = txQueue.poll() ?: break
                            sock.send(DatagramPacket(pkt, pkt.size, addr, UDP_REMOTE_PORT))
                        }
                        try {
                            val rx = DatagramPacket(buf, buf.size)
                            sock.receive(rx)
                            handleIncomingPacket(rx.data, rx.length)
                        } catch (_: Exception) {
                        }
                        ticks++
                        if (ticks % 80 == 0) {
                            sock.send(DatagramPacket(keepAlive, keepAlive.size, addr, UDP_REMOTE_PORT))
                        }
                    }
                }
            } catch (e: Exception) {
                withContext(Dispatchers.Main) {
                    isConnected = false
                    connectionStatus = "Erro UDP: ${e.message}"
                }
            }
        }
    }

    fun connectTcp(ip: String) {
        disconnect()
        connectIp = ip.trim()
        saveUserPrefs()
        netRunning.set(true)
        isConnected = true
        connectionStatus = "TCP $connectIp:$TCP_REMOTE_PORT"

        viewModelScope.launch(Dispatchers.IO) {
            val keepAlive = AxiosProtocol.buildKvFrame("DEVNAME=\n", false)
            while (netRunning.get()) {
                try {
                    Socket().use { s ->
                        s.connect(InetSocketAddress(connectIp, TCP_REMOTE_PORT), 2000)
                        s.soTimeout = 30
                        s.tcpNoDelay = true
                        val inp = s.getInputStream()
                        val out = s.getOutputStream()
                        val buf = ByteArray(512)
                        var ticks = 0

                        readAllRegistersFromHardware()

                        while (netRunning.get()) {
                            while (true) {
                                val pkt = txQueue.poll() ?: break
                                out.write(pkt)
                            }
                            try {
                                val n = inp.read(buf)
                                if (n > 0) handleIncomingPacket(buf, n)
                                else if (n < 0) break
                            } catch (_: Exception) {
                            }
                            ticks++
                            if (ticks % 80 == 0) out.write(keepAlive)
                        }
                    }
                } catch (_: Exception) {
                    delay(1200)
                }
            }
        }
    }

    fun disconnect() {
        netRunning.set(false)
        txQueue.clear()
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

    private suspend fun handleIncomingPacket(data: ByteArray, length: Int) {
        val parsed = AxiosProtocol.parseFrame(data, length) ?: return
        val (cmd, payload) = parsed
        if (cmd == 0x03 || cmd == 0x06) {
            val updates = ArrayList<Pair<Int, Int>>()
            var i = 0
            while (i + 4 <= payload.size) {
                val r = ((payload[i].toInt() and 0xFF) shl 8) or (payload[i + 1].toInt() and 0xFF)
                val v = ((payload[i + 2].toInt() and 0xFF) shl 8) or (payload[i + 3].toInt() and 0xFF)
                updates.add(r to v)
                i += 4
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
