package com.duonn.axios24

import android.content.res.Configuration
import android.os.Build
import android.os.Bundle
import android.view.WindowManager
import androidx.activity.ComponentActivity
import androidx.activity.compose.setContent
import androidx.activity.viewModels
import androidx.core.view.WindowCompat
import androidx.core.view.WindowInsetsCompat
import androidx.core.view.WindowInsetsControllerCompat
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.horizontalScroll
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.platform.LocalConfiguration
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import kotlin.math.roundToInt

class MainActivity : ComponentActivity() {
    private val vm: AxiosViewModel by viewModels()

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        enableImmersiveFullscreen()
        setContent {
            MaterialTheme(
                colorScheme = darkColorScheme(
                    primary = Color(0xFF00C8F5),
                    secondary = Color(0xFF2ECC71),
                    background = Color(0xFF0B0F17),
                    surface = Color(0xFF141B29)
                )
            ) {
                Surface(
                    modifier = Modifier.fillMaxSize(),
                    color = Color(0xFF0B0F17)
                ) {
                    Box(
                        modifier = Modifier
                            .fillMaxSize()
                            .safeDrawingPadding()
                    ) {
                        AxiosAppScreen(vm)
                    }
                }
            }
        }
    }

    override fun onWindowFocusChanged(hasFocus: Boolean) {
        super.onWindowFocusChanged(hasFocus)
        if (hasFocus) {
            enableImmersiveFullscreen()
        }
    }

    private fun enableImmersiveFullscreen() {
        WindowCompat.setDecorFitsSystemWindows(window, false)
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.P) {
            val attrs = window.attributes
            attrs.layoutInDisplayCutoutMode =
                WindowManager.LayoutParams.LAYOUT_IN_DISPLAY_CUTOUT_MODE_SHORT_EDGES
            window.attributes = attrs
        }
        val controller = WindowInsetsControllerCompat(window, window.decorView)
        controller.systemBarsBehavior =
            WindowInsetsControllerCompat.BEHAVIOR_SHOW_TRANSIENT_BARS_BY_SWIPE
        controller.hide(WindowInsetsCompat.Type.systemBars())
    }

    override fun onStop() {
        super.onStop()
        vm.saveUserPrefs()
    }
}

fun formatDb(db: Float): String =
    if (db <= -115f) "-∞ dB" else String.format("%+.1f dB", db)

@Composable
fun AxiosAppScreen(vm: AxiosViewModel) {
    // Observe uiTick to recompose when registers change
    val tick = vm.uiTick
    val isLandscape = LocalConfiguration.current.orientation == Configuration.ORIENTATION_LANDSCAPE

    if (vm.personalMonitorLocked) {
        PersonalMonitorLockedScreen(vm)
        return
    }

    Column(modifier = Modifier.fillMaxSize().padding(6.dp)) {
        TopHeaderBar(vm, tick)
        Spacer(Modifier.height(4.dp))

        Box(modifier = Modifier.weight(1f).fillMaxWidth()) {
            when (vm.activeTab) {
                MainTab.MIXER -> MixerScreen(vm, isLandscape, tick)
                MainTab.EDIT -> EditChannelScreen(vm, isLandscape, tick)
                MainTab.EQ -> EqScreen(vm, tick)
                MainTab.FX_BT -> FxAndBluetoothScreen(vm, tick)
                MainTab.SCENES -> ScenesAndNetworkScreen(vm)
            }
        }

        Spacer(Modifier.height(4.dp))
        BottomNavBar(vm)
    }
}

@Composable
fun TopHeaderBar(vm: AxiosViewModel, tick: Int) {
    val _t = tick
    val sel = vm.selectedChannel
    val lrMuted = vm.channelMuted(SelectedChannel.Output(8)) || vm.channelMuted(SelectedChannel.Output(9))
    val allUnplugged = vm.allSafeUnplugged()

    Column(
        modifier = Modifier
            .fillMaxWidth()
            .background(Color(0xFF121826), RoundedCornerShape(6.dp))
            .border(1.dp, Color(0xFF25334A), RoundedCornerShape(6.dp))
            .padding(horizontal = 8.dp, vertical = 5.dp)
    ) {
        Row(
            modifier = Modifier.fillMaxWidth().horizontalScroll(rememberScrollState()),
            verticalAlignment = Alignment.CenterVertically,
            horizontalArrangement = Arrangement.spacedBy(6.dp)
        ) {
            Text(
                text = "AXIOS 24 PRO",
                color = Color(0xFF00D8FF),
                fontWeight = FontWeight.Bold,
                fontSize = 13.sp
            )
            PillChip(
                text = if (vm.isConnected) "ONLINE: ${vm.connectIp}" else "OFFLINE (Conectar)",
                active = vm.isConnected,
                activeColor = Color(0xFF27AE60),
                onClick = { vm.activeTab = MainTab.SCENES }
            )
            PillChip(
                text = "Sel: ${vm.channelName(sel)} (${sel.shortBadge()})",
                active = true,
                activeColor = Color(0xFF0097B2),
                onClick = { vm.activeTab = MainTab.EDIT }
            )
            PillChip(
                text = "HPF 100Hz",
                active = false,
                activeColor = Color(0xFF2ECC71),
                onClick = { vm.toggleQuickHpf100Hz(sel) }
            )
            PillChip(
                text = if (allUnplugged) "Desplugar ON" else "Desplugar Todos",
                active = allUnplugged,
                activeColor = Color(0xFFE67E22),
                onClick = { vm.toggleSafeUnplugAll() }
            )
            val offLr = vm.disconnectedLrCount()
            PillChip(
                text = if (offLr > 0) "Liberar Todos L&R ($offLr off)" else "L&R: Todos ON",
                active = offLr == 0,
                activeColor = if (offLr > 0) Color(0xFFF39C12) else Color(0xFF27AE60),
                onClick = { vm.routeAllInputsToMainLr() }
            )
            if (lrMuted) {
                PillChip(
                    text = "L/R MUTADO (Liberar)",
                    active = true,
                    activeColor = Color(0xFFE74C3C),
                    onClick = {
                        vm.setChannelMuted(SelectedChannel.Output(8), false)
                        vm.setChannelMuted(SelectedChannel.Output(9), false)
                    }
                )
            }
            vm.statusToast?.let { msg ->
                Text(
                    text = msg,
                    color = Color(0xFF2ECC71),
                    fontSize = 11.sp,
                    maxLines = 1,
                    overflow = TextOverflow.Ellipsis
                )
            }
        }
    }
}

@Composable
fun BottomNavBar(vm: AxiosViewModel) {
    Row(
        modifier = Modifier
            .fillMaxWidth()
            .background(Color(0xFF121826), RoundedCornerShape(6.dp))
            .padding(4.dp),
        horizontalArrangement = Arrangement.spacedBy(4.dp)
    ) {
        for (tab in MainTab.entries) {
            val active = vm.activeTab == tab
            Box(
                modifier = Modifier
                    .weight(1f)
                    .clip(RoundedCornerShape(5.dp))
                    .background(if (active) Color(0xFF0097B2) else Color(0xFF1B2436))
                    .clickable { vm.activeTab = tab }
                    .padding(vertical = 8.dp),
                contentAlignment = Alignment.Center
            ) {
                Text(
                    text = tab.label,
                    color = if (active) Color.White else Color(0xFFB5C4D8),
                    fontSize = 12.sp,
                    fontWeight = FontWeight.Bold,
                    maxLines = 1
                )
            }
        }
    }
}

@Composable
fun MixerScreen(vm: AxiosViewModel, isLandscape: Boolean, tick: Int) {
    val _t = tick
    Column(modifier = Modifier.fillMaxSize()) {
        // Layer + Sends on Fader selector bar
        Row(
            modifier = Modifier.fillMaxWidth().horizontalScroll(rememberScrollState()),
            horizontalArrangement = Arrangement.spacedBy(5.dp),
            verticalAlignment = Alignment.CenterVertically
        ) {
            for (ly in FaderLayer.entries) {
                PillChip(
                    text = ly.label,
                    active = vm.layer == ly,
                    activeColor = Color(0xFF00A8CC),
                    onClick = { vm.layer = ly }
                )
            }
            Spacer(Modifier.width(6.dp))
            PillChip(
                text = "SOF: MAIN",
                active = vm.sendsOnFader is SendsOnFaderTarget.MainLR,
                activeColor = Color(0xFF27AE60),
                onClick = { vm.sendsOnFader = SendsOnFaderTarget.MainLR }
            )
            for (aux in 0..7) {
                PillChip(
                    text = "AUX${aux + 1}",
                    active = vm.sendsOnFader == SendsOnFaderTarget.Aux(aux),
                    activeColor = Color(0xFFE67E22),
                    onClick = { vm.sendsOnFader = SendsOnFaderTarget.Aux(aux) }
                )
            }
        }

        Spacer(Modifier.height(6.dp))

        // Always-visible Bluetooth / DIGI + MAIN L/R quick bar
        QuickMasterAndBtBar(vm, tick)

        Spacer(Modifier.height(6.dp))

        val channels = vm.layer.channels()
        if (isLandscape) {
            // Horizontal (Landscape): side-by-side vertical channel strips
            Row(
                modifier = Modifier
                    .fillMaxSize()
                    .horizontalScroll(rememberScrollState()),
                horizontalArrangement = Arrangement.spacedBy(6.dp)
            ) {
                for (sel in channels) {
                    LandscapeChannelCard(vm, sel, tick)
                }
            }
        } else {
            // Vertical (Portrait): touch-friendly scrollable list of channel rows for easy 1-hand operation
            LazyColumn(
                modifier = Modifier.fillMaxSize(),
                verticalArrangement = Arrangement.spacedBy(6.dp)
            ) {
                items(channels) { sel ->
                    PortraitChannelRow(vm, sel, tick)
                }
            }
        }
    }
}

@Composable
fun QuickMasterAndBtBar(vm: AxiosViewModel, tick: Int) {
    val _t = tick
    val digiSel = SelectedChannel.Input(24)
    val mainLSel = SelectedChannel.Output(8)
    val digiDb = vm.channelFaderDb(digiSel)
    val mainDb = vm.channelFaderDb(mainLSel)
    val digiMuted = vm.channelMuted(digiSel)
    val mainMuted = vm.channelMuted(mainLSel)

    Row(
        modifier = Modifier
            .fillMaxWidth()
            .background(Color(0xFF141D2B), RoundedCornerShape(6.dp))
            .border(1.dp, Color(0xFF283A52), RoundedCornerShape(6.dp))
            .padding(horizontal = 8.dp, vertical = 4.dp),
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.spacedBy(8.dp)
    ) {
        PillChip(
            text = if (digiMuted) "BT/DIGI MUT" else "BT/DIGI ON",
            active = !digiMuted,
            activeColor = Color(0xFF0097B2),
            onClick = { vm.setChannelMuted(digiSel, !digiMuted) }
        )
        Slider(
            value = digiDb,
            onValueChange = { vm.setChannelFaderDb(digiSel, it) },
            valueRange = -80f..10f,
            modifier = Modifier.weight(1f).height(26.dp)
        )
        Text(formatDb(digiDb), color = Color.White, fontSize = 11.sp)

        PillChip(
            text = if (mainMuted) "MAIN MUT" else "MAIN L&R",
            active = !mainMuted,
            activeColor = Color(0xFFE74C3C),
            onClick = { vm.setChannelMuted(mainLSel, !mainMuted) }
        )
        Slider(
            value = mainDb,
            onValueChange = { vm.setChannelFaderDb(mainLSel, it) },
            valueRange = -80f..10f,
            modifier = Modifier.weight(1f).height(26.dp)
        )
        Text(formatDb(mainDb), color = Color(0xFF00E5FF), fontSize = 11.sp, fontWeight = FontWeight.Bold)
        VuMeterBar(
            levelDb = vm.channelMeterDb(mainLSel),
            clipped = vm.channelClipped(mainLSel),
            onClearClip = { vm.clearClipHold(mainLSel) },
            segments = 10,
            modifier = Modifier.width(66.dp).height(10.dp)
        )
    }
}

@Composable
fun VuMeterBar(
    levelDb: Float,
    clipped: Boolean,
    onClearClip: () -> Unit,
    segments: Int = 18,
    modifier: Modifier = Modifier.fillMaxWidth().height(8.dp)
) {
    val norm = ((levelDb + 60f) / 60f).coerceIn(0f, 1f)
    val litCount = (norm * segments).roundToInt()
    Row(
        modifier = modifier.clickable(onClick = onClearClip),
        horizontalArrangement = Arrangement.spacedBy(2.dp),
        verticalAlignment = Alignment.CenterVertically
    ) {
        for (i in 0 until segments) {
            val frac = (i + 1).toFloat() / segments.toFloat()
            val lit = i < litCount
            val segColor = when {
                frac > 0.85f -> if (lit || (clipped && i == segments - 1)) Color(0xFFFF3B30) else Color(0xFF3A1A1A)
                frac > 0.65f -> if (lit) Color(0xFFFFCC00) else Color(0xFF3A3318)
                else -> if (lit) Color(0xFF2ECC71) else Color(0xFF173024)
            }
            Box(
                modifier = Modifier
                    .weight(1f)
                    .fillMaxHeight()
                    .clip(RoundedCornerShape(2.dp))
                    .background(segColor)
            )
        }
    }
}

@Composable
fun PortraitChannelRow(vm: AxiosViewModel, sel: SelectedChannel, tick: Int) {
    val _t = tick
    val isSel = vm.selectedChannel == sel
    val faderDb = vm.effectiveFaderDb(sel)
    val muted = vm.channelMuted(sel)
    val solo = vm.channelSolo(sel)
    val (grp, ch) = sel.groupAndIndex()

    Column(
        modifier = Modifier
            .fillMaxWidth()
            .background(if (isSel) Color(0xFF1B283C) else Color(0xFF131B28), RoundedCornerShape(6.dp))
            .border(1.dp, if (isSel) Color(0xFF00D8FF) else Color(0xFF243248), RoundedCornerShape(6.dp))
            .clickable { vm.selectedChannel = sel }
            .padding(8.dp)
    ) {
        Row(
            modifier = Modifier.fillMaxWidth(),
            verticalAlignment = Alignment.CenterVertically,
            horizontalArrangement = Arrangement.spacedBy(6.dp)
        ) {
            Text(
                text = "${sel.shortBadge()} • ${vm.channelName(sel)}",
                color = Color.White,
                fontWeight = FontWeight.Bold,
                fontSize = 13.sp,
                modifier = Modifier.weight(1f)
            )
            if (grp == 1 && ch <= 24) {
                val toLr = vm.inputToMainLr(ch)
                PillChip(
                    text = "L&R",
                    active = toLr,
                    activeColor = Color(0xFF2ECC71),
                    onClick = { vm.setInputToMainLr(ch, !toLr) }
                )
            }
            if (grp == 1 && ch < 24) {
                Text(
                    text = "Gain +${vm.inputPreampGainDb(ch).roundToInt()}dB",
                    color = Color(0xFFF39C12),
                    fontSize = 11.sp
                )
                PillChip(
                    text = if (vm.safeUnplugged[ch]) "PLUGAR" else "DESPLUG",
                    active = vm.safeUnplugged[ch],
                    activeColor = Color(0xFFE67E22),
                    onClick = { vm.toggleSafeUnplug(ch) }
                )
            }
            PillChip(
                text = "0dB",
                active = false,
                activeColor = Color(0xFF3498DB),
                onClick = { vm.setEffectiveFaderDb(sel, 0f) }
            )
            PillChip(
                text = "S",
                active = solo,
                activeColor = Color(0xFFF1C40F),
                onClick = { vm.setChannelSolo(sel, !solo) }
            )
            PillChip(
                text = if (muted) "MUTADO" else "MUTE",
                active = muted,
                activeColor = Color(0xFFE74C3C),
                onClick = { vm.setChannelMuted(sel, !muted) }
            )
        }
        Spacer(Modifier.height(4.dp))
        VuMeterBar(
            levelDb = vm.channelMeterDb(sel),
            clipped = vm.channelClipped(sel),
            onClearClip = { vm.clearClipHold(sel) },
            segments = 24,
            modifier = Modifier.fillMaxWidth().height(7.dp)
        )
        Row(verticalAlignment = Alignment.CenterVertically) {
            Slider(
                value = faderDb.coerceIn(-90f, 10f),
                onValueChange = { vm.setEffectiveFaderDb(sel, if (it <= -88f) -120f else it) },
                valueRange = -90f..10f,
                modifier = Modifier.weight(1f)
            )
            Spacer(Modifier.width(6.dp))
            Text(
                text = formatDb(faderDb),
                color = Color(0xFF00E5FF),
                fontWeight = FontWeight.Bold,
                fontSize = 12.sp,
                modifier = Modifier.width(64.dp)
            )
        }
    }
}

@Composable
fun LandscapeChannelCard(vm: AxiosViewModel, sel: SelectedChannel, tick: Int) {
    val _t = tick
    val isSel = vm.selectedChannel == sel
    val faderDb = vm.effectiveFaderDb(sel)
    val muted = vm.channelMuted(sel)
    val solo = vm.channelSolo(sel)
    val (grp, ch) = sel.groupAndIndex()

    Column(
        modifier = Modifier
            .width(118.dp)
            .fillMaxHeight()
            .background(if (isSel) Color(0xFF1B283C) else Color(0xFF131B28), RoundedCornerShape(6.dp))
            .border(1.dp, if (isSel) Color(0xFF00D8FF) else Color(0xFF243248), RoundedCornerShape(6.dp))
            .clickable { vm.selectedChannel = sel }
            .padding(6.dp),
        horizontalAlignment = Alignment.CenterHorizontally,
        verticalArrangement = Arrangement.SpaceBetween
    ) {
        Text(
            text = vm.channelName(sel),
            color = Color.White,
            fontWeight = FontWeight.Bold,
            fontSize = 12.sp,
            maxLines = 1,
            overflow = TextOverflow.Ellipsis
        )
        Text(text = sel.shortBadge(), color = Color(0xFF8AA4C0), fontSize = 10.sp)

        if (grp == 1 && ch < 24) {
            val g = vm.inputPreampGainDb(ch)
            Text("Gain +${g.roundToInt()}dB", color = Color(0xFFF39C12), fontSize = 10.sp)
        }

        Row(horizontalArrangement = Arrangement.spacedBy(4.dp)) {
            PillChip("M", muted, Color(0xFFE74C3C)) { vm.setChannelMuted(sel, !muted) }
            PillChip("S", solo, Color(0xFFF1C40F)) { vm.setChannelSolo(sel, !solo) }
            if (grp == 1 && ch <= 24) {
                val toLr = vm.inputToMainLr(ch)
                PillChip("L&R", toLr, Color(0xFF2ECC71)) { vm.setInputToMainLr(ch, !toLr) }
            } else {
                PillChip("0dB", false, Color(0xFF3498DB)) { vm.setEffectiveFaderDb(sel, 0f) }
            }
        }

        VuMeterBar(
            levelDb = vm.channelMeterDb(sel),
            clipped = vm.channelClipped(sel),
            onClearClip = { vm.clearClipHold(sel) },
            segments = 12,
            modifier = Modifier.fillMaxWidth().height(8.dp)
        )

        Slider(
            value = faderDb.coerceIn(-90f, 10f),
            onValueChange = { vm.setEffectiveFaderDb(sel, if (it <= -88f) -120f else it) },
            valueRange = -90f..10f,
            modifier = Modifier.fillMaxWidth()
        )

        Text(
            text = formatDb(faderDb),
            color = Color(0xFF00E5FF),
            fontWeight = FontWeight.Bold,
            fontSize = 12.sp
        )
    }
}

@Composable
fun EditChannelScreen(vm: AxiosViewModel, isLandscape: Boolean, tick: Int) {
    val _t = tick
    val sel = vm.selectedChannel
    val (grp, ch) = sel.groupAndIndex()
    var nameBuf by remember(sel) { mutableStateOf(vm.channelName(sel)) }

    Column(
        modifier = Modifier
            .fillMaxSize()
            .verticalScroll(rememberScrollState())
            .padding(6.dp),
        verticalArrangement = Arrangement.spacedBy(8.dp)
    ) {
        // Channel selector strip
        Row(
            modifier = Modifier.fillMaxWidth().horizontalScroll(rememberScrollState()),
            horizontalArrangement = Arrangement.spacedBy(4.dp)
        ) {
            for (i in 0 until 25) {
                val c = SelectedChannel.Input(i)
                PillChip(c.shortBadge(), sel == c, Color(0xFF00A8CC)) { vm.selectedChannel = c }
            }
            for (o in 0..9) {
                val c = SelectedChannel.Output(o)
                PillChip(c.shortBadge(), sel == c, Color(0xFF9B59B6)) { vm.selectedChannel = c }
            }
        }

        // Rename + Presets
        Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(6.dp)) {
            OutlinedTextField(
                value = nameBuf,
                onValueChange = {
                    nameBuf = it
                    vm.setChannelName(sel, it)
                },
                label = { Text("Nome do Canal (${sel.shortBadge()})") },
                singleLine = true,
                modifier = Modifier.weight(1f)
            )
            PillChip("HPF 100Hz", false, Color(0xFF2ECC71)) { vm.toggleQuickHpf100Hz(sel) }
        }

        if (grp == 1 && ch < 24) {
            Text("Presets Rápidos de Instrumento/Voz:", color = Color(0xFF00D8FF), fontWeight = FontWeight.Bold, fontSize = 12.sp)
            Row(
                modifier = Modifier.fillMaxWidth().horizontalScroll(rememberScrollState()),
                horizontalArrangement = Arrangement.spacedBy(6.dp)
            ) {
                for (p in ChannelPreset.entries) {
                    PillChip(p.label, false, Color(0xFF8E44AD)) { vm.applyChannelPreset(sel, p) }
                }
            }

            // Preamp, +48V, Phase, Delay (ms & m)
            Card(colors = CardDefaults.cardColors(containerColor = Color(0xFF141D2B))) {
                Column(Modifier.padding(10.dp), verticalArrangement = Arrangement.spacedBy(6.dp)) {
                    Text("1. PREAMP, GANHO & DELAY (${String.format("%.2f m", vm.inputDelayMs(ch) * 0.343f)})", color = Color(0xFF00E5FF), fontWeight = FontWeight.Bold)
                    Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                        val p48 = vm.inputPhantom48v(ch)
                        PillChip("+48V Phantom", p48, Color(0xFFE74C3C)) { vm.setInputPhantom48v(ch, !p48) }
                        val inv = vm.inputPhaseInvert(ch)
                        PillChip("Ø Fase", inv, Color(0xFFF39C12)) { vm.setInputPhaseInvert(ch, !inv) }
                        val lr = vm.inputToMainLr(ch)
                        PillChip("Envio L/R", lr, Color(0xFF2ECC71)) { vm.setInputToMainLr(ch, !lr) }
                    }
                    val gain = vm.inputPreampGainDb(ch)
                    Text("Ganho Preamp: +${gain.roundToInt()} dB", color = Color.White, fontSize = 12.sp)
                    Slider(value = gain, onValueChange = { vm.setInputPreampGainDb(ch, it) }, valueRange = 0f..60f)

                    val dly = vm.inputDelayMs(ch)
                    Text("Delay: ${String.format("%.1f ms = %.2f m", dly, dly * 0.343f)}", color = Color.White, fontSize = 12.sp)
                    Slider(value = dly, onValueChange = { vm.setInputDelayMs(ch, it) }, valueRange = 0f..10f)
                }
            }

            // Noise Gate & Compressor
            Card(colors = CardDefaults.cardColors(containerColor = Color(0xFF141D2B))) {
                Column(Modifier.padding(10.dp), verticalArrangement = Arrangement.spacedBy(6.dp)) {
                    Text("2. NOISE GATE & COMPRESSOR", color = Color(0xFFF39C12), fontWeight = FontWeight.Bold)
                    val gOn = vm.gateEnabled(ch)
                    val gTh = vm.gateThresholdDb(ch)
                    Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                        PillChip(if (gOn) "Gate ON" else "Gate OFF", gOn, Color(0xFF2ECC71)) { vm.setGateEnabled(ch, !gOn) }
                        Text("Threshold Gate: ${gTh.roundToInt()} dB", color = Color.White, fontSize = 12.sp)
                    }
                    Slider(value = gTh, onValueChange = { vm.setGateThresholdDb(ch, it) }, valueRange = -80f..0f)

                    val cOn = vm.compEnabled(1, ch)
                    val cTh = vm.compThresholdDb(1, ch)
                    val cRt = vm.compRatio(1, ch)
                    Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                        PillChip(if (cOn) "Compressor ON" else "Compressor OFF", cOn, Color(0xFFF39C12)) { vm.setCompEnabled(1, ch, !cOn) }
                        Text("Th: ${cTh.roundToInt()} dB | Ratio 1:${String.format("%.1f", cRt)}", color = Color.White, fontSize = 12.sp)
                    }
                    Slider(value = cTh, onValueChange = { vm.setCompThresholdDb(1, ch, it) }, valueRange = -60f..0f)
                    Slider(value = cRt, onValueChange = { vm.setCompRatio(1, ch, it) }, valueRange = 1f..20f)
                }
            }

            // Aux Sends 1-8 & FX 1-2
            Card(colors = CardDefaults.cardColors(containerColor = Color(0xFF141D2B))) {
                Column(Modifier.padding(10.dp), verticalArrangement = Arrangement.spacedBy(4.dp)) {
                    Text("3. MANDADAS AUX 1-8 & FX 1-2", color = Color(0xFF2ECC71), fontWeight = FontWeight.Bold)
                    for (aux in 0..7) {
                        val sDb = vm.inputAuxSendDb(ch, aux)
                        Row(verticalAlignment = Alignment.CenterVertically) {
                            Text("AUX ${aux + 1}", color = Color.White, fontSize = 12.sp, modifier = Modifier.width(52.dp))
                            Slider(
                                value = sDb.coerceIn(-90f, 10f),
                                onValueChange = { vm.setInputAuxSendDb(ch, aux, if (it <= -88f) -120f else it) },
                                valueRange = -90f..10f,
                                modifier = Modifier.weight(1f)
                            )
                            Text(formatDb(sDb), color = Color(0xFF00E5FF), fontSize = 11.sp, modifier = Modifier.width(58.dp))
                        }
                    }
                }
            }
        }
    }
}

@Composable
fun EqScreen(vm: AxiosViewModel, tick: Int) {
    val _t = tick
    val sel = vm.selectedChannel
    val (rawG, rawC) = sel.groupAndIndex()
    val g = rawG.coerceIn(1, 3)
    val c = if (rawG == 4) 0 else rawC
    val bands = if (g == 3) 7 else 4
    val eqOn = vm.eqEnabled(g, c)
    val hpfOn = vm.hpfEnabled(g, c)
    val hpfHz = vm.hpfFreqHz(g, c)

    Column(
        modifier = Modifier
            .fillMaxSize()
            .verticalScroll(rememberScrollState())
            .padding(6.dp),
        verticalArrangement = Arrangement.spacedBy(8.dp)
    ) {
        Row(
            modifier = Modifier.fillMaxWidth().horizontalScroll(rememberScrollState()),
            horizontalArrangement = Arrangement.spacedBy(6.dp),
            verticalAlignment = Alignment.CenterVertically
        ) {
            Text("EQ PARAMÉTRICO — ${vm.channelName(sel)}", color = Color(0xFF00E5FF), fontWeight = FontWeight.Bold)
            PillChip(if (eqOn) "PEQ ATIVO" else "PEQ BYPASS", eqOn, Color(0xFF2ECC71)) { vm.setEqEnabled(g, c, !eqOn) }
            PillChip(if (hpfOn) "HPF ON (${hpfHz.roundToInt()}Hz)" else "HPF OFF", hpfOn, Color(0xFFF39C12)) { vm.setHpfEnabled(g, c, !hpfOn) }
            PillChip("HPF 100Hz", false, Color(0xFF00A8CC)) { vm.toggleQuickHpf100Hz(sel) }
        }

        Card(colors = CardDefaults.cardColors(containerColor = Color(0xFF141D2B))) {
            Column(Modifier.padding(10.dp)) {
                Text("Corte de Graves (HPF): ${hpfHz.roundToInt()} Hz", color = Color.White, fontSize = 12.sp)
                Slider(value = hpfHz, onValueChange = { vm.setHpfFreqHz(g, c, it) }, valueRange = 20f..400f)
            }
        }

        for (b in 0 until bands) {
            val gain = vm.peqBandGainDb(g, c, b)
            val freq = vm.peqBandFreqHz(g, c, b)
            Card(colors = CardDefaults.cardColors(containerColor = Color(0xFF141D2B))) {
                Column(Modifier.padding(10.dp)) {
                    Row(
                        modifier = Modifier.fillMaxWidth(),
                        horizontalArrangement = Arrangement.SpaceBetween
                    ) {
                        Text("Banda ${b + 1}: ${freq.roundToInt()} Hz", color = Color(0xFF00E5FF), fontWeight = FontWeight.Bold)
                        Text(String.format("%+.1f dB", gain), color = Color.White, fontWeight = FontWeight.Bold)
                    }
                    Text("Ganho (-15 a +15 dB)", color = Color(0xFF9AB0C8), fontSize = 11.sp)
                    Slider(value = gain, onValueChange = { vm.setPeqBandGainDb(g, c, b, it) }, valueRange = -12f..12f)
                    Text("Frequência (20 Hz a 20 kHz)", color = Color(0xFF9AB0C8), fontSize = 11.sp)
                    Slider(value = freq, onValueChange = { vm.setPeqBandFreqHz(g, c, b, it) }, valueRange = 20f..16000f)
                }
            }
        }
    }
}

@Composable
fun FxAndBluetoothScreen(vm: AxiosViewModel, tick: Int) {
    val _t = tick
    val digiSel = SelectedChannel.Input(24)
    val digiDb = vm.channelFaderDb(digiSel)
    val digiMuted = vm.channelMuted(digiSel)

    Column(
        modifier = Modifier
            .fillMaxSize()
            .verticalScroll(rememberScrollState())
            .padding(6.dp),
        verticalArrangement = Arrangement.spacedBy(10.dp)
    ) {
        Card(colors = CardDefaults.cardColors(containerColor = Color(0xFF141D2B))) {
            Column(Modifier.padding(12.dp), verticalArrangement = Arrangement.spacedBy(8.dp)) {
                Text("GERENCIADOR BLUETOOTH & ENTRADA DIGITAL (DIGI CH 25-26)", color = Color(0xFF00E5FF), fontWeight = FontWeight.Bold)
                Row(
                    modifier = Modifier.fillMaxWidth().horizontalScroll(rememberScrollState()),
                    horizontalArrangement = Arrangement.spacedBy(6.dp)
                ) {
                    val sources = listOf(0 to "USB Music", 1 to "Bluetooth", 2 to "Optical", 3 to "Coaxial")
                    for ((id, lbl) in sources) {
                        PillChip(lbl, vm.mediaSourceId == id, Color(0xFF0097B2)) { vm.setMediaSource(id) }
                    }
                }
                PillChip(
                    text = "Ativar Bluetooth + Liberar Playback (0.0 dB / L&R)",
                    active = vm.mediaSourceId == 1 && !digiMuted,
                    activeColor = Color(0xFF27AE60),
                    onClick = { vm.enableBluetoothPlaybackQuick() }
                )
                VuMeterBar(
                    levelDb = vm.channelMeterDb(digiSel),
                    clipped = vm.channelClipped(digiSel),
                    onClearClip = { vm.clearClipHold(digiSel) },
                    segments = 20,
                    modifier = Modifier.fillMaxWidth().height(8.dp)
                )
                Row(verticalAlignment = Alignment.CenterVertically) {
                    Text("Volume BT/DIGI:", color = Color.White, modifier = Modifier.width(110.dp))
                    Slider(
                        value = digiDb.coerceIn(-90f, 10f),
                        onValueChange = { vm.setChannelFaderDb(digiSel, if (it <= -88f) -120f else it) },
                        valueRange = -90f..10f,
                        modifier = Modifier.weight(1f)
                    )
                    Text(formatDb(digiDb), color = Color(0xFF00E5FF), fontWeight = FontWeight.Bold)
                }
            }
        }

        for (fx in 0..1) {
            val fxSel = SelectedChannel.Fx(fx)
            val fxDb = vm.channelFaderDb(fxSel)
            val fxMuted = vm.channelMuted(fxSel)
            Card(colors = CardDefaults.cardColors(containerColor = Color(0xFF141D2B))) {
                Column(Modifier.padding(12.dp), verticalArrangement = Arrangement.spacedBy(6.dp)) {
                    Row(
                        modifier = Modifier.fillMaxWidth(),
                        horizontalArrangement = Arrangement.SpaceBetween,
                        verticalAlignment = Alignment.CenterVertically
                    ) {
                        Text("PROCESSADOR INTERNO FX ${fx + 1}", color = Color(0xFFE91E63), fontWeight = FontWeight.Bold)
                        PillChip(if (fxMuted) "FX MUTADO" else "FX ATIVO", !fxMuted, Color(0xFFE91E63)) {
                            vm.setChannelMuted(fxSel, !fxMuted)
                        }
                    }
                    Row(verticalAlignment = Alignment.CenterVertically) {
                        Text("Nível FX -> L/R:", color = Color.White, modifier = Modifier.width(110.dp))
                        Slider(
                            value = fxDb.coerceIn(-90f, 10f),
                            onValueChange = { vm.setChannelFaderDb(fxSel, if (it <= -88f) -120f else it) },
                            valueRange = -90f..10f,
                            modifier = Modifier.weight(1f)
                        )
                        Text(formatDb(fxDb), color = Color.White)
                    }
                }
            }
        }
    }
}

@Composable
fun ScenesAndNetworkScreen(vm: AxiosViewModel) {
    Column(
        modifier = Modifier
            .fillMaxSize()
            .verticalScroll(rememberScrollState())
            .padding(6.dp),
        verticalArrangement = Arrangement.spacedBy(10.dp)
    ) {
        // Auto-Discovery & Network Card
        Card(colors = CardDefaults.cardColors(containerColor = Color(0xFF141D2B))) {
            Column(Modifier.padding(12.dp), verticalArrangement = Arrangement.spacedBy(8.dp)) {
                Text("CONEXÃO AUTOMÁTICA NA REDE INTERNA (WI-FI / LAN)", color = Color(0xFF00E5FF), fontWeight = FontWeight.Bold)
                Text("IP deste Android: ${vm.localPhoneIp} | Status: ${vm.connectionStatus}", color = Color(0xFFB5C4D8), fontSize = 12.sp)

                Row(horizontalArrangement = Arrangement.spacedBy(6.dp)) {
                    for (fw in FirmwareLayout.entries) {
                        PillChip(fw.label, vm.firmwareLayout == fw, Color(0xFF0097B2)) {
                            vm.firmwareLayout = fw
                            vm.saveUserPrefs()
                        }
                    }
                }

                Row(horizontalArrangement = Arrangement.spacedBy(6.dp)) {
                    PillChip(
                        text = if (vm.isScanning) "Buscando..." else "Buscar Mesa na Rede (Auto)",
                        active = true,
                        activeColor = Color(0xFF00A8CC),
                        onClick = { vm.scanNetworkDevices() }
                    )
                    if (vm.isConnected) {
                        PillChip("Ler Cena da Mesa", false, Color(0xFF2ECC71)) { vm.readAllRegistersFromHardware() }
                        PillChip("Desconectar", false, Color(0xFFE74C3C)) { vm.disconnect() }
                    }
                }

                if (vm.discoveredDevices.isNotEmpty()) {
                    for (dev in vm.discoveredDevices) {
                        Row(
                            modifier = Modifier
                                .fillMaxWidth()
                                .background(Color(0xFF1B283C), RoundedCornerShape(5.dp))
                                .padding(8.dp),
                            horizontalArrangement = Arrangement.SpaceBetween,
                            verticalAlignment = Alignment.CenterVertically
                        ) {
                            Text("${dev.name} — ${dev.ip} (${dev.fwVersion})", color = Color(0xFF2ECC71), fontWeight = FontWeight.Bold)
                            PillChip("Conectar Agora", true, Color(0xFF27AE60)) { vm.connectUdp(dev.ip) }
                        }
                    }
                }

                Row(
                    modifier = Modifier.fillMaxWidth().horizontalScroll(rememberScrollState()),
                    horizontalArrangement = Arrangement.spacedBy(6.dp)
                ) {
                    for ((ip, desc) in DUONN_FACTORY_IPS) {
                        PillChip("$ip ($desc)", vm.connectIp == ip, Color(0xFF34495E)) { vm.connectIp = ip }
                    }
                }

                Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(6.dp)) {
                    OutlinedTextField(
                        value = vm.connectIp,
                        onValueChange = { vm.connectIp = it },
                        label = { Text("IP da Mesa Axios 24") },
                        singleLine = true,
                        modifier = Modifier.weight(1f)
                    )
                    PillChip("UDP (8086)", vm.isConnected, Color(0xFF27AE60)) { vm.connectUdp(vm.connectIp) }
                    PillChip("TCP (8087)", false, Color(0xFF2980B9)) { vm.connectTcp(vm.connectIp) }
                }
                Text(
                    text = "Diagnóstico VU/Rede: ${vm.lastUnknownPacketInfo}",
                    color = Color(0xFF94A3B8),
                    fontSize = 11.sp
                )
            }
        }

        // Personal Monitor Mode ("Meu Fone")
        Card(colors = CardDefaults.cardColors(containerColor = Color(0xFF141D2B))) {
            Column(Modifier.padding(12.dp), verticalArrangement = Arrangement.spacedBy(8.dp)) {
                Text("MODO MONITOR PESSOAL (\"MEU FONE\" TRAVADO)", color = Color(0xFF2ECC71), fontWeight = FontWeight.Bold)
                Row(
                    modifier = Modifier.fillMaxWidth().horizontalScroll(rememberScrollState()),
                    horizontalArrangement = Arrangement.spacedBy(6.dp)
                ) {
                    for (aux in 0..7) {
                        PillChip("AUX ${aux + 1}", vm.personalMonitorAux == aux, Color(0xFF16A085)) {
                            vm.personalMonitorAux = aux
                        }
                    }
                }
                PillChip(
                    text = "Travar Tela \"Meu Fone\" (Só AUX ${vm.personalMonitorAux + 1})",
                    active = false,
                    activeColor = Color(0xFFE67E22),
                    onClick = { vm.personalMonitorLocked = true }
                )
            }
        }

        // 16 Scenes Recall / Save
        Card(colors = CardDefaults.cardColors(containerColor = Color(0xFF141D2B))) {
            Column(Modifier.padding(12.dp), verticalArrangement = Arrangement.spacedBy(6.dp)) {
                Text("16 CENAS DA MESA (RECALL & SAVE)", color = Color(0xFFF39C12), fontWeight = FontWeight.Bold)
                for (s in 0 until 16) {
                    Row(
                        modifier = Modifier
                            .fillMaxWidth()
                            .background(Color(0xFF1A2436), RoundedCornerShape(5.dp))
                            .padding(8.dp),
                        horizontalArrangement = Arrangement.SpaceBetween,
                        verticalAlignment = Alignment.CenterVertically
                    ) {
                        Text(String.format("%02d. %s", s + 1, vm.sceneNames[s]), color = Color.White, fontWeight = FontWeight.Bold)
                        Row(horizontalArrangement = Arrangement.spacedBy(6.dp)) {
                            PillChip("Carregar", false, Color(0xFF0097B2)) { vm.recallScene(s) }
                            PillChip("Salvar", false, Color(0xFFE67E22)) { vm.saveScene(s) }
                        }
                    }
                }
            }
        }
    }
}

@Composable
fun PersonalMonitorLockedScreen(vm: AxiosViewModel) {
    val aux = vm.personalMonitorAux
    Column(modifier = Modifier.fillMaxSize().padding(8.dp)) {
        Row(
            modifier = Modifier.fillMaxWidth(),
            horizontalArrangement = Arrangement.SpaceBetween,
            verticalAlignment = Alignment.CenterVertically
        ) {
            Text(
                text = "MEU FONE (TRAVADO) — AUX ${aux + 1}",
                color = Color(0xFF2ECC71),
                fontWeight = FontWeight.Bold,
                fontSize = 16.sp
            )
            Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(6.dp)) {
                OutlinedTextField(
                    value = vm.unlockPinInput,
                    onValueChange = { vm.unlockPinInput = it },
                    placeholder = { Text("Senha (1234)") },
                    singleLine = true,
                    modifier = Modifier.width(120.dp)
                )
                PillChip("Sair", false, Color(0xFFE74C3C)) {
                    if (vm.unlockPinInput == "1234") {
                        vm.personalMonitorLocked = false
                        vm.unlockPinInput = ""
                    } else {
                        vm.toast("Senha padrão: 1234")
                    }
                }
            }
        }
        Spacer(Modifier.height(6.dp))
        LazyColumn(verticalArrangement = Arrangement.spacedBy(6.dp)) {
            items((0..24).toList()) { ch ->
                val sel = SelectedChannel.Input(ch)
                val db = vm.inputAuxSendDb(ch, aux)
                Row(
                    modifier = Modifier
                        .fillMaxWidth()
                        .background(Color(0xFF141D2B), RoundedCornerShape(6.dp))
                        .padding(8.dp),
                    verticalAlignment = Alignment.CenterVertically
                ) {
                    Text(
                        text = "${sel.shortBadge()} ${vm.channelName(sel)}",
                        color = Color.White,
                        fontWeight = FontWeight.Bold,
                        modifier = Modifier.width(110.dp)
                    )
                    Slider(
                        value = db.coerceIn(-90f, 10f),
                        onValueChange = { vm.setInputAuxSendDb(ch, aux, if (it <= -88f) -120f else it) },
                        valueRange = -90f..10f,
                        modifier = Modifier.weight(1f)
                    )
                    Text(formatDb(db), color = Color(0xFF00E5FF), modifier = Modifier.width(62.dp))
                }
            }
        }
    }
}

@Composable
fun PillChip(
    text: String,
    active: Boolean,
    activeColor: Color,
    onClick: () -> Unit
) {
    Box(
        modifier = Modifier
            .clip(RoundedCornerShape(5.dp))
            .background(if (active) activeColor else Color(0xFF1E293B))
            .border(1.dp, if (active) Color.White else activeColor.copy(alpha = 0.6f), RoundedCornerShape(5.dp))
            .clickable(onClick = onClick)
            .padding(horizontal = 10.dp, vertical = 6.dp),
        contentAlignment = Alignment.Center
    ) {
        Text(
            text = text,
            color = Color.White,
            fontSize = 11.sp,
            fontWeight = FontWeight.Bold,
            maxLines = 1
        )
    }
}
