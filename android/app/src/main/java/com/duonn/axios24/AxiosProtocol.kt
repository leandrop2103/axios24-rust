package com.duonn.axios24

import java.io.InputStream
import java.io.OutputStream
import java.net.DatagramPacket
import java.net.DatagramSocket
import java.net.Inet4Address
import java.net.InetAddress
import java.net.InetSocketAddress
import java.net.NetworkInterface
import java.net.Socket
import java.util.concurrent.LinkedBlockingQueue
import java.util.concurrent.atomic.AtomicBoolean

const val NUM_REGS = 0xD55 // 3413 legacy registers
const val AX24E_NUM_REGS = 0xDBE // 3518 AX24E 25.11.01.001 registers
const val UDP_REMOTE_PORT = 8086
const val TCP_REMOTE_PORT = 8087

val DUONN_FACTORY_IPS = listOf(
    "172.14.10.1" to "Axios 24 (Padrão Novo)",
    "192.168.4.1" to "Axios 24 (AP Legado)",
    "172.14.11.1" to "Axios 16",
    "172.14.30.1" to "Axios 32"
)

enum class FirmwareLayout(val label: String, val numRegs: Int) {
    AX24E_2511("AX24E 25.11.01.001 (3518 regs)", AX24E_NUM_REGS),
    LEGACY("Legado v1.0.25.0328 (3413 regs)", NUM_REGS);

    fun toDevice(reg: Int): Int = when (this) {
        LEGACY -> reg
        AX24E_2511 -> when {
            reg < 0xB2F -> reg
            reg <= 0xC00 -> reg + 0x24
            else -> reg + 0x69
        }
    }

    fun fromDevice(reg: Int): Int? {
        val r = when (this) {
            LEGACY -> reg
            AX24E_2511 -> when {
                reg < 0xB2F -> reg
                reg < 0xB53 -> return null
                reg <= 0xC24 -> reg - 0x24
                reg < 0xC6A -> return null
                else -> reg - 0x69
            }
        }
        return if (r in 0 until NUM_REGS) r else null
    }

    companion object {
        fun detect(devName: String, fwVer: String): FirmwareLayout? {
            val s = "$devName $fwVer".uppercase()
            return when {
                s.contains("AX24E") || s.contains("AXIOS24E") || s.contains("25.11") -> AX24E_2511
                s.contains("1.0.25.0328") -> LEGACY
                else -> null
            }
        }
    }
}

data class DiscoveredDevice(
    val name: String,
    val ip: String,
    val fwVersion: String
)

object AxiosProtocol {
    private val SCN_XOR_KEY = intArrayOf(
        0x7d, 0x85, 0xe9, 0x07, 0xec, 0xf3, 0x32, 0xb9, 0x74, 0xa2, 0x37, 0x92, 0x1c, 0x37, 0x04, 0x2c,
        0x5c, 0x3e, 0xf2, 0xaf, 0x80, 0x83, 0xa0, 0xfa, 0xe1, 0x2d, 0x03, 0xcb, 0xaa, 0xae, 0x02, 0x8f,
        0x49, 0x4e, 0xe5, 0x42, 0x14, 0xec, 0xb8, 0xe2, 0xbe, 0x6b, 0x43, 0x15, 0x2e, 0x8b, 0x1d, 0x01,
        0x30, 0xb2, 0x89, 0x22, 0x3d, 0x8d, 0x9e, 0xd0, 0x14, 0xd1, 0xf8, 0x68, 0xc5, 0xec, 0x69, 0x67,
        0x39, 0xe6, 0x1c, 0xfe, 0x93, 0x48, 0x44, 0x02, 0xdd, 0x7b, 0x8c, 0x67, 0xc9, 0xcc, 0xb6, 0x56,
        0x62, 0x84, 0x69, 0x7f, 0x18, 0x28, 0xcb, 0xbd, 0xc3, 0x06, 0x32, 0x83, 0xfd, 0xa2, 0x8b, 0x9a,
        0xa4, 0x5e, 0x80, 0x6b, 0x36, 0x21, 0x5a, 0x82, 0x4e, 0xbf, 0x1d, 0x58, 0x46, 0xf2, 0xb8, 0x5e,
        0xb2, 0xe5, 0x5d, 0xeb, 0x69, 0x6c, 0x57, 0x04, 0xa4, 0x5f, 0x2a, 0x21, 0xa9, 0x11, 0x68, 0xa2
    )

    /** Exact CRC-16 (Modbus polynomial 0xA001, init 0xFFFF) from fcn.0045a3b0 */
    fun crc16Axios(data: ByteArray, length: Int = data.size): Int {
        var crc = 0xFFFF
        for (i in 0 until length) {
            crc = crc xor (data[i].toInt() and 0xFF)
            for (bit in 0 until 8) {
                crc = if ((crc and 1) != 0) {
                    (crc ushr 1) xor 0xA001
                } else {
                    crc ushr 1
                }
            }
        }
        return crc and 0xFFFF
    }

    /** Builds a framed Axios 24 packet: [0x80, payload.size + 3, cmd, ...payload, crc_hi, crc_lo] */
    fun buildFrame(cmd: Int, payload: ByteArray): ByteArray {
        val out = ByteArray(payload.size + 5)
        out[0] = 0x80.toByte()
        out[1] = (payload.size + 3).toByte()
        out[2] = cmd.toByte()
        System.arraycopy(payload, 0, out, 3, payload.size)
        val crc = crc16Axios(out, payload.size + 3)
        out[payload.size + 3] = ((crc ushr 8) and 0xFF).toByte()
        out[payload.size + 4] = (crc and 0xFF).toByte()
        return out
    }

    /** Verifies a framed Axios 24 packet and returns Pair(cmd, payload) */
    fun parseFrame(frame: ByteArray, length: Int = frame.size): Pair<Int, ByteArray>? {
        if (length < 5 || (frame[0].toInt() and 0xFF) != 0x80) return null
        val b1 = frame[1].toInt() and 0xFF
        val b2 = frame[2].toInt() and 0xFF
        val crcOffset = if (b1 == 0xFF && b2 == 0xFF) 0x106 else b1
        if (length < crcOffset + 2 || crcOffset < 3) return null
        val expected = crc16Axios(frame, crcOffset)
        val actual = ((frame[crcOffset].toInt() and 0xFF) shl 8) or (frame[crcOffset + 1].toInt() and 0xFF)
        if (expected != actual) return null
        val cmd = frame[2].toInt() and 0xFF
        val payload = frame.copyOfRange(3, crcOffset)
        return cmd to payload
    }

    fun buildParamWriteFrame(pairs: List<Pair<Int, Int>>): ByteArray {
        val count = pairs.size.coerceAtMost(0x3C)
        val payload = ByteArray(count * 4)
        for (i in 0 until count) {
            val (reg, v) = pairs[i]
            payload[i * 4] = ((reg ushr 8) and 0xFF).toByte()
            payload[i * 4 + 1] = (reg and 0xFF).toByte()
            payload[i * 4 + 2] = ((v ushr 8) and 0xFF).toByte()
            payload[i * 4 + 3] = (v and 0xFF).toByte()
        }
        return buildFrame(0x03, payload)
    }

    fun buildParamReadFrame(regs: List<Int>): ByteArray {
        val count = regs.size.coerceAtMost(0x3C)
        val payload = ByteArray(count * 2)
        for (i in 0 until count) {
            val reg = regs[i]
            payload[i * 2] = ((reg ushr 8) and 0xFF).toByte()
            payload[i * 2 + 1] = (reg and 0xFF).toByte()
        }
        return buildFrame(0x06, payload)
    }

    fun buildSceneOpFrame(scene1Based: Int, save: Boolean): ByteArray =
        buildFrame(if (save) 0x11 else 0x01, byteArrayOf(scene1Based.toByte()))

    fun buildKvFrame(text: String, isSet: Boolean): ByteArray =
        buildFrame(if (isSet) 0x3A else 0x39, text.toByteArray(Charsets.UTF_8))

    /** Digital / Bluetooth source switch frame (opcode 0x72) */
    fun buildMediaSourceFrame(sourceId: Int): ByteArray {
        val payload = when (sourceId) {
            0 -> byteArrayOf(0x00, 0x00, 0xA8.toByte()) // USB Music / Player
            1 -> byteArrayOf(0x00, 0x00, 0xC7.toByte()) // Bluetooth
            2 -> byteArrayOf(0x01, 0x00, 0xC3.toByte()) // Optical
            3 -> byteArrayOf(0x02, 0x00, 0xC3.toByte()) // Coaxial
            else -> byteArrayOf(0x00, 0x00, 0xB5.toByte()) // USB Audio
        }
        return buildFrame(0x72, payload)
    }

    /** Exact register mapping function from fcn.00455d70 */
    fun mapReg(group: Int, ch: Int, param: Int): Int {
        if (param == 0xEC) {
            return when (group) {
                1 -> 0x57 + 0x3E * ch
                2 -> 0xB3B + 0x2D * ch
                3 -> 0x698 + 0x6D * ch
                4 -> 0xBAA + 9 * ch
                5 -> 0xAEB + 5 * ch + param
                6 -> 0xAFB + 3 * ch + param
                else -> 0xC0F
            }
        } else if (param == 0xEB) {
            return when (group) {
                1 -> 0x4A + 0x3E * ch
                2 -> 0xB30 + 0x2D * ch
                3 -> 0x68E + 0x6D * ch
                4 -> 0xBA9 + 9 * ch
                5 -> 0xAEB + 5 * ch + param
                6 -> 0xAFB + 3 * ch + param
                else -> 0xC0F
            }
        } else if (param == 0xEA) {
            return when (group) {
                1 -> 0x4C + 0x3E * ch
                2 -> 0xB2F + 0x2D * ch
                3 -> 0x68C + 0x6D * ch
                4 -> 0xBA8 + 9 * ch
                5 -> 0xAEB + 5 * ch + param
                6 -> 0xAFB + 3 * ch + param
                else -> 0xC0F
            }
        }
        return when (group) {
            1 -> ch * 0x3E + param + 0x40
            2 -> if (param == 0xD8) ch + 0xBF5 else if (param in 0xAB until 0xD8) ch * 0x2D + param + 0xA84 else 0xC0F
            3 -> ch * 0x6D + param + 0x64E
            4 -> ch * 9 + param + 0xACE
            5 -> ch * 5 + param + 0xAEB
            6 -> ch * 3 + param + 0xAFB
            else -> 0xC0F
        }
    }

    data class OfficialScn(val name: String, val channelNames: List<String>, val regs: IntArray)

    fun decodeOfficialScn(bytes: ByteArray, defaultRegs: IntArray): OfficialScn? {
        val dec = ByteArray(bytes.size) { i ->
            (bytes[i].toInt() xor SCN_XOR_KEY[i % SCN_XOR_KEY.size]).toByte()
        }
        val json = dec.toString(Charsets.UTF_8).trimStart()
        if (!json.startsWith("{")) return null
        val dataHex = Regex("\"data\"\\s*:\\s*\"([0-9A-Fa-f]+)\"").find(json)?.groupValues?.get(1) ?: return null
        val pairs = ArrayList<Pair<Int, Int>>(dataHex.length / 8)
        var i = 0
        while (i + 8 <= dataHex.length) {
            val r = dataHex.substring(i, i + 4).toIntOrNull(16)
            val v = dataHex.substring(i + 4, i + 8).toIntOrNull(16)
            if (r != null && v != null) pairs.add(r to v)
            i += 8
        }
        if (pairs.size < 100) return null
        val layout = if (pairs.any { it.first >= NUM_REGS }) FirmwareLayout.AX24E_2511 else FirmwareLayout.LEGACY
        val regs = defaultRegs.copyOf()
        for ((r, v) in pairs) {
            val mapped = layout.fromDevice(r)
            if (mapped != null) regs[mapped] = v
        }
        val name = Regex("\"name\"\\s*:\\s*\"([^\"]*)\"").find(json)?.groupValues?.get(1) ?: "Importada"
        val textBlock = Regex("\"text\"\\s*:\\s*\\[(.*?)\\]").find(json)?.groupValues?.get(1) ?: ""
        val names = Regex("\"([^\"]*)\"").findAll(textBlock).map { it.groupValues[1] }.toList()
        return OfficialScn(name, names, regs)
    }

    fun encodeOfficialScn(
        sceneNum: Int,
        sceneName: String,
        channelNames: List<String>,
        regs: IntArray,
        layout: FirmwareLayout
    ): ByteArray {
        val n = layout.numRegs
        val dev = IntArray(n)
        if (layout == FirmwareLayout.AX24E_2511) {
            for (ch in 0 until 26) dev[0xB2F + ch] = ch + 1
            for (out in 0 until 10) dev[0xB49 + out] = out + 1
        }
        for (r in regs.indices) {
            val d = layout.toDevice(r)
            if (d in 0 until n) dev[d] = regs[r]
        }
        val sb = StringBuilder(n * 8)
        for (r in 0 until n) {
            sb.append(String.format("%04X%04X", r, dev[r] and 0xFFFF))
        }
        val textJson = (0 until 60).joinToString(",") { idx ->
            val clean = (channelNames.getOrNull(idx) ?: "").replace("\\", "\\\\").replace("\"", "\\\"")
            "\"$clean\""
        }
        val cleanScene = sceneName.replace("\\", "\\\\").replace("\"", "\\\"")
        val json = "{\"code\":\"0xE001\",\"type\":\"0x21\",\"magic\":28581,\"scenes\":[{\"num\":$sceneNum,\"name\":\"$cleanScene\",\"text\":[$textJson],\"data\":\"$sb\",\"magic\":584}]}"
        val raw = json.toByteArray(Charsets.UTF_8)
        return ByteArray(raw.size) { idx ->
            (raw[idx].toInt() xor SCN_XOR_KEY[idx % SCN_XOR_KEY.size]).toByte()
        }
    }

    fun detectLocalIpv4(): String? {
        return try {
            NetworkInterface.getNetworkInterfaces()?.toList()?.flatMap { it.inetAddresses.toList() }
                ?.firstOrNull { !it.isLoopbackAddress && it is Inet4Address }?.hostAddress
        } catch (_: Exception) {
            null
        }
    }

    /** Automatic LAN & Wi-Fi UDP discovery on port 8086 using official 0x00 (80 04 00 00 25 68) + 0x39 probes */
    fun probeUdpDevices(): List<DiscoveredDevice> {
        val found = ArrayList<DiscoveredDevice>()
        try {
            DatagramSocket().use { sock ->
                sock.broadcast = true
                sock.soTimeout = 350
                val probeOfficial = buildFrame(0x00, byteArrayOf(0x00)) // Exact 80 04 00 00 25 68
                val probeKv = buildKvFrame("DEVNAME=\nFW_VER=\nFW_BUILD=\n", false)
                val targets = linkedSetOf("255.255.255.255")
                detectLocalIpv4()?.let { ip ->
                    val parts = ip.split(".")
                    if (parts.size == 4) targets.add("${parts[0]}.${parts[1]}.${parts[2]}.255")
                }
                for ((ip, _) in DUONN_FACTORY_IPS) targets.add(ip)

                for (t in targets) {
                    try {
                        val addr = InetAddress.getByName(t)
                        sock.send(DatagramPacket(probeOfficial, probeOfficial.size, addr, UDP_REMOTE_PORT))
                        sock.send(DatagramPacket(probeKv, probeKv.size, addr, UDP_REMOTE_PORT))
                    } catch (_: Exception) {
                    }
                }

                val buf = ByteArray(512)
                val start = System.currentTimeMillis()
                while (System.currentTimeMillis() - start < 900) {
                    try {
                        val pkt = DatagramPacket(buf, buf.size)
                        sock.receive(pkt)
                        val parsed = parseFrame(pkt.data, pkt.length)
                        if (parsed != null && (parsed.first == 0x00 || parsed.first == 0x39)) {
                            val cmd = parsed.first
                            val pl = parsed.second
                            var name = "AXIOS24"
                            var fw = "V1.0"
                            if (cmd == 0x00 && pl.size >= 5) {
                                val model = ((pl[1].toInt() and 0xFF) shl 8) or (pl[2].toInt() and 0xFF)
                                val nameBytes = pl.copyOfRange(5, pl.size)
                                val nul = nameBytes.indexOf(0.toByte()).let { if (it >= 0) it else nameBytes.size }
                                val parsedName = nameBytes.copyOfRange(0, nul).toString(Charsets.UTF_8).trim()
                                name = if (parsedName.isEmpty() || parsedName.equals("unnamed", ignoreCase = true)) "AXIOS24E" else parsedName
                                fw = if (model == 0xE001) "AX24E.25.11.01.001" else String.format("0x%04X", model)
                            } else {
                                val txt = pl.toString(Charsets.UTF_8)
                                for (line in txt.lines()) {
                                    if (line.startsWith("DEVNAME=")) {
                                        val v = line.removePrefix("DEVNAME=").trim()
                                        if (v.isNotEmpty()) name = v
                                    } else if (line.startsWith("FW_VER=")) {
                                        fw = line.removePrefix("FW_VER=").trim()
                                    }
                                }
                            }
                            val ip = pkt.address.hostAddress ?: continue
                            if (found.none { it.ip == ip }) {
                                found.add(DiscoveredDevice(name, ip, fw))
                            }
                        }
                    } catch (_: Exception) {
                        break
                    }
                }
            }
        } catch (_: Exception) {
        }
        return found
    }
}
