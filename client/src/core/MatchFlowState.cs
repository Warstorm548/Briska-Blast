namespace BriskaBlast.Core;

/// <summary>Where the client is in the session lifecycle. Exactly one state at a
/// time; every change goes through <see cref="MatchFlow"/>'s transition gate.</summary>
public enum MatchFlowState
{
    /// <summary>No session. Main menu / setup screens.</summary>
    Idle,
    /// <summary>In a lobby with a live signaling socket, waiting for Start.</summary>
    InLobby,
    /// <summary>Match starting (or rejoining one): WebRTC mesh coming up behind
    /// the "Connecting to players…" screen. Ends in InMatch or a clean failure.</summary>
    Preparing,
    /// <summary>Playing. GameScene is up and the mesh carries handoffs.</summary>
    InMatch,
    /// <summary>Match over (GameOver received). The end-game screen owns
    /// navigation; the session teardown that follows is expected.</summary>
    PostMatch,
}
