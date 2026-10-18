package io.github.mtmattia.rondine.ui

import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.navigationBarsPadding
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.lazy.grid.GridCells
import androidx.compose.foundation.lazy.grid.LazyVerticalGrid
import androidx.compose.foundation.lazy.grid.items
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp

/** Le emoji più usate, nell'ordine in cui di solito si cercano. */
private val EMOJI = """
😀 😃 😄 😁 😆 😅 😂 🤣 🥲 😊 😇 🙂 🙃 😉 😌 😍 🥰 😘 😗 😙 😚 😋 😛 😝 😜 🤪 🤨 🧐 🤓 😎
🥳 😏 😒 😞 😔 😟 😕 🙁 😣 😖 😫 😩 🥺 😢 😭 😤 😠 😡 🤯 😳 🥵 🥶 😱 😨 😰 😥 😓 🤗 🤔 🤭
🤫 🤥 😶 😐 😑 😬 🙄 😯 😦 😧 😮 😲 🥱 😴 🤤 😪 😵 🤐 🥴 🤢 🤮 🤧 😷 🤒 🤕 🤑 🤠 😈 👻 💩
👍 👎 👌 🤌 ✌️ 🤞 🤟 🤘 🤙 👈 👉 👆 👇 ☝️ ✋ 🤚 👋 👏 🙌 🙏 🤝 💪 ✍️ 👀 🫶
❤️ 🧡 💛 💚 💙 💜 🖤 🤍 🤎 💔 ❣️ 💕 💞 💓 💗 💖 💘 💝 💯 ✨ 🔥 ⭐ 🌟 🎉 🎊 🎁 🎂 🍕 🍝 🍷
☕ 🍺 🥂 🍦 🍰 🌹 🌻 🌸 🌈 ☀️ 🌙 ⛅ 🌧️ ❄️ 🐶 🐱 🐦 🕊️ 🐝 🦋 🚗 ✈️ 🏠 ⚽ 🎵 📷 📞 ⏰ ✅ ❌
""".trim().split(Regex("\\s+"))

/** Pannello delle emoji, al posto della tastiera. */
@Composable
fun EmojiPanel(onEmoji: (String) -> Unit) {
    Surface(color = MaterialTheme.colorScheme.surface, modifier = Modifier.fillMaxWidth()) {
        LazyVerticalGrid(
            GridCells.Adaptive(44.dp),
            Modifier.height(280.dp).padding(horizontal = 6.dp).navigationBarsPadding(),
        ) {
            items(EMOJI) { e ->
                Box(
                    Modifier.height(48.dp).clickable { onEmoji(e) },
                    contentAlignment = Alignment.Center,
                ) { Text(e, fontSize = 26.sp) }
            }
        }
    }
}
