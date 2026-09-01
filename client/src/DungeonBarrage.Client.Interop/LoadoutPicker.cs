using DungeonBarrage.Client.Contracts;

namespace DungeonBarrage.Client.Interop;

/// <summary>
/// The loadout picker the Godot select screen drives: clicking (or focusing) an item tile
/// equips that item into its slot. Confirm reads <see cref="Loadout"/>; it does not keep a
/// separate default triangle.
/// </summary>
public sealed class LoadoutPicker
{
    private readonly IReadOnlyList<ClientItemDefinition> _items;
    private int _mainIndex;
    private int _secondaryIndex;
    private int _meleeIndex;

    /// <summary>Creates a picker over a catalog, defaulting each slot to the launch triangle when present.</summary>
    /// <param name="items">Catalog items. Must contain at least one entry per slot.</param>
    public LoadoutPicker(IReadOnlyList<ClientItemDefinition> items)
    {
        ArgumentNullException.ThrowIfNull(items);
        if (items.Count == 0)
        {
            throw new ArgumentException("The item catalog must not be empty.", nameof(items));
        }

        _items = items;
        _mainIndex = IndexOfSlot(items, ClientAbilitySlot.Main, "ramshot-cannon");
        _secondaryIndex = IndexOfSlot(items, ClientAbilitySlot.Secondary, "recurve-bow");
        _meleeIndex = IndexOfSlot(items, ClientAbilitySlot.MeleeTool, "trench-spade");
        FocusedIndex = _mainIndex;
    }

    /// <summary>Catalog the picker was constructed with.</summary>
    public IReadOnlyList<ClientItemDefinition> Items => _items;

    /// <summary>Tile currently focused by keyboard or last click.</summary>
    public int FocusedIndex { get; private set; }

    /// <summary>Tile index equipped in the main slot.</summary>
    public int MainIndex => _mainIndex;

    /// <summary>Tile index equipped in the secondary slot.</summary>
    public int SecondaryIndex => _secondaryIndex;

    /// <summary>Tile index equipped in the melee/tool slot.</summary>
    public int MeleeIndex => _meleeIndex;

    /// <summary>The loadout Confirm will put on the create envelope.</summary>
    public ClientLoadout Loadout =>
        new(IdAt(_mainIndex), IdAt(_secondaryIndex), IdAt(_meleeIndex));

    /// <summary>
    /// Focuses <paramref name="index"/> and equips that tile into the slot its item occupies.
    /// A main-slot item replaces only main; secondary and melee are left alone.
    /// </summary>
    /// <param name="index">Catalog index of the clicked or keyboard-selected tile.</param>
    public void SelectTile(int index)
    {
        if (index < 0 || index >= _items.Count)
        {
            return;
        }

        FocusedIndex = index;
        switch (_items[index].Slot)
        {
            case ClientAbilitySlot.Main:
                _mainIndex = index;
                break;
            case ClientAbilitySlot.Secondary:
                _secondaryIndex = index;
                break;
            case ClientAbilitySlot.MeleeTool:
                _meleeIndex = index;
                break;
        }
    }

    /// <summary>Whether <paramref name="index"/> is one of the three equipped tiles.</summary>
    /// <param name="index">Catalog index.</param>
    /// <returns><see langword="true"/> if that tile is currently equipped.</returns>
    public bool IsEquipped(int index) =>
        index == _mainIndex || index == _secondaryIndex || index == _meleeIndex;

    private string IdAt(int index) =>
        index >= 0 && index < _items.Count ? _items[index].Id : string.Empty;

    private static int IndexOfSlot(
        IReadOnlyList<ClientItemDefinition> items,
        ClientAbilitySlot slot,
        string preferredId)
    {
        for (var i = 0; i < items.Count; i++)
        {
            if (items[i].Slot == slot && items[i].Id == preferredId)
            {
                return i;
            }
        }

        for (var i = 0; i < items.Count; i++)
        {
            if (items[i].Slot == slot)
            {
                return i;
            }
        }

        // The constructor documents "at least one entry per slot" as a precondition, so a
        // catalog missing one is a broken catalog, not a case to paper over. Falling back to
        // index 0 here used to hand every such slot whatever item happened to be first —
        // silently building a loadout whose secondary or melee id is a main-slot item, which
        // the native side then rejects far from the actual cause.
        throw new ArgumentException(
            $"The item catalog has no entry for the {slot} slot.", nameof(items));
    }
}
