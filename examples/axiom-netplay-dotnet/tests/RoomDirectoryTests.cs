using Axiom.Netplay;
using Xunit;

namespace Axiom.Netplay.Tests;

/// <summary>Phase D — the scaleout room directory: it spreads rooms across nodes,
/// keeps a room pinned to one node, and requires at least one registered node.</summary>
public sealed class RoomDirectoryTests
{
    private static InMemoryRoomDirectory TwoNodeDirectory()
    {
        var dir = new InMemoryRoomDirectory(capacity: 2);
        dir.RegisterNode("ws://node-a/ws");
        dir.RegisterNode("ws://node-b/ws");
        return dir;
    }

    [Fact]
    public void assignment_spreads_rooms_across_nodes()
    {
        var dir = TwoNodeDirectory();

        (string NodeUrl, string RoomId)[] assigns =
            Enumerable.Range(0, 5).Select(_ => dir.AssignMatch()).ToArray();

        // Both nodes are used — rooms are distributed, not piled onto one node.
        Assert.Equal(2, assigns.Select(a => a.NodeUrl).Distinct().Count());
        // Five tickets into 2-player rooms ⇒ three rooms.
        Assert.Equal(3, dir.RoomCount);
    }

    [Fact]
    public void a_room_is_always_pinned_to_the_same_node()
    {
        var dir = TwoNodeDirectory();
        (string NodeUrl, string RoomId) first = dir.AssignMatch();
        (string NodeUrl, string RoomId) second = dir.AssignMatch();

        // The first two tickets share a room (capacity 2) — same room, same node.
        Assert.Equal(first.RoomId, second.RoomId);
        Assert.Equal(first.NodeUrl, second.NodeUrl);
    }

    [Fact]
    public void registration_is_idempotent_and_required()
    {
        var dir = new InMemoryRoomDirectory(capacity: 2);
        Assert.Throws<InvalidOperationException>(() => dir.AssignMatch()); // no nodes yet

        dir.RegisterNode("ws://node-a/ws");
        dir.RegisterNode("ws://node-a/ws"); // duplicate ignored
        Assert.Single(dir.Nodes);
        Assert.Equal("ws://node-a/ws", dir.AssignMatch().NodeUrl);
    }
}
