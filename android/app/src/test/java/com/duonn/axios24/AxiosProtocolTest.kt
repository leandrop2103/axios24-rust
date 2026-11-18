package com.duonn.axios24

import org.junit.Assert.assertArrayEquals
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNotNull
import org.junit.Test

class AxiosProtocolTest {
    @Test
    fun testFrameBuildParseAndAx24eLayout() {
        val frame = AxiosProtocol.buildParamWriteFrame(listOf(0x0057 to 0x04B0))
        val parsed = AxiosProtocol.parseFrame(frame)
        assertNotNull(parsed)
        assertEquals(0x03, parsed!!.first)
        assertArrayEquals(byteArrayOf(0x00, 0x57, 0x04, 0xB0.toByte()), parsed.second)

        val btFrame = AxiosProtocol.buildMediaSourceFrame(1)
        val btParsed = AxiosProtocol.parseFrame(btFrame)
        assertNotNull(btParsed)
        assertEquals(0x72, btParsed!!.first)
        assertArrayEquals(byteArrayOf(0x00, 0x00, 0xC7.toByte()), btParsed.second)

        val ax = FirmwareLayout.AX24E_2511
        assertEquals(0x0057, ax.toDevice(AxiosProtocol.mapReg(1, 0, 0xEC)))
        assertEquals(0x0A00, ax.toDevice(AxiosProtocol.mapReg(3, 8, 0xEC)))
        assertEquals(0x0B5F, ax.toDevice(AxiosProtocol.mapReg(2, 0, 0xEC)))
        assertEquals(0x0BCE, ax.toDevice(AxiosProtocol.mapReg(4, 0, 0xEC)))
        assertEquals(0x0BF0, ax.toDevice(0x0BCC))
    }
}
